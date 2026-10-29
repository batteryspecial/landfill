const WS: u8 = 0b0000_0011;
const TOKEN: u8 = 0b0111_1100;
const PUNCT: u8 = 0b1000_0000;

const LO: [u8; 16] = {
    let mut t = [0; 16];
    let mut l = 0;
    while l < 16 {
        let mut c = 0;
        if matches!(l, 0x9 | 0xB | 0xC | 0xD) {
            c |= 1 << 0; 
        }
        if l == 0 {
            c |= 1 << 1;
        }
        if l <= 9 {
            c |= 1 << 2; 
        }
        if l >= 1 {
            c |= 1 << 3; 
        }
        if l <= 0xA {
            c |= 1 << 4;
        }
        if l == 0xF {
            c |= 1 << 5;
        }
        if matches!(l, 0xB | 0xD) {
            c |= 1 << 6;
        }
        if matches!(l, 0x8 | 0x9 | 0xC) {
            c |= 1 << 7;
        }
        t[l] = c;
        l += 1;
    }
    t
};

const HI: [u8; 16] = {
    let mut t = [0; 16];

    t[0] = 1 << 0;
    t[2] = 1 << 1 | 1 << 6 | 1 << 7;
    t[3] = 1 << 2;
    t[4] = 1 << 3;
    t[5] = 1 << 4 | 1 << 5;
    t[6] = 1 << 3;
    t[7] = 1 << 4;
    t
};

#[derive(Clone, Copy)]
pub(super) struct Masks {
    pub ws: u32,
    pub tok: u32,
    pub punct: u32
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub(super) fn masks32(w: &[u8]) -> Masks {
    use std::arch::aarch64::*;
    assert!(w.len() >= 32);
    // SAFETY: NEON is baseline on aarch64, and w holds at least 32 bytes
    unsafe {
        let lo = vld1q_u8(LO.as_ptr());
        let hi = vld1q_u8(HI.as_ptr());
        let class = |v: uint8x16_t| {
            vandq_u8(vqtbl1q_u8(lo, vandq_u8(v, vdupq_n_u8(0xF))), vqtbl1q_u8(hi, vshrq_n_u8::<4>(v)))
        };
        let (ra, rb) = (vld1q_u8(w.as_ptr()), vld1q_u8(w.as_ptr().add(16)));
        let (a, b) = (class(ra), class(rb));
        let colon = vdupq_n_u8(b':');

        // one byte per char -> one bit per char: weight each lane, then pairwise-add down
        const BITS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
        let bits = vld1q_u8(BITS.as_ptr());
        let pick = |v: uint8x16_t, k: u8| vandq_u8(vtstq_u8(v, vdupq_n_u8(k)), bits);
        let punct = |v: uint8x16_t, r: uint8x16_t| {
            vandq_u8(vorrq_u8(vtstq_u8(v, vdupq_n_u8(PUNCT)), vceqq_u8(r, colon)), bits)
        };
        let ws = vpaddq_u8(pick(a, WS), pick(b, WS));
        let tok = vpaddq_u8(pick(a, TOKEN), pick(b, TOKEN));
        let pun = vpaddq_u8(punct(a, ra), punct(b, rb));
        let x = vreinterpretq_u32_u8(vpaddq_u8(vpaddq_u8(ws, tok), vpaddq_u8(pun, pun)));
        Masks { ws: vgetq_lane_u32::<0>(x), tok: vgetq_lane_u32::<1>(x), punct: vgetq_lane_u32::<2>(x) }
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
pub(super) fn masks32(w: &[u8]) -> Masks {
    // scalar fallback, add SSE/AVX2 if x86 throughput matters
    let mut m = Masks { ws: 0, tok: 0, punct: 0 };
    for (k, &c) in w[..32].iter().enumerate() {
        let class = LO[(c & 0xF) as usize] & HI[(c >> 4) as usize];
        m.ws |= ((class & WS != 0) as u32) << k;
        m.tok |= ((class & TOKEN != 0) as u32) << k;
        m.punct |= ((class & PUNCT != 0 || c == b':') as u32) << k;
    }
    m
}

// Bit k set where b[p + k] == '\n'; b must hold 64 bytes from p
#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn newlines64(b: &[u8], p: usize) -> u64 {
    use std::arch::aarch64::*;
    let w = &b[p..p + 64];
    // SAFETY: NEON is baseline on aarch64, and w holds 64 bytes
    unsafe {
        const BITS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
        let bits = vld1q_u8(BITS.as_ptr());
        let nl = vdupq_n_u8(b'\n');
        let at = |k: usize| vandq_u8(vceqq_u8(vld1q_u8(w.as_ptr().add(16 * k)), nl), bits);
        let x = vpaddq_u8(vpaddq_u8(at(0), at(1)), vpaddq_u8(at(2), at(3)));
        vgetq_lane_u64::<0>(vreinterpretq_u64_u8(vpaddq_u8(x, x)))
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn newlines64(b: &[u8], p: usize) -> u64 {
    b[p..p + 64].iter().enumerate().fold(0, |m, (k, &c)| m | ((c == b'\n') as u64) << k)
}

pub(super) const LINE_BATCH: usize = 512;            // fits in L1
pub(super) type Ends = [usize; LINE_BATCH + 64];

// Fill out with the start of each line after from; returns how many.
pub(super) fn line_ends(b: &[u8], from: usize, out: &mut Ends) -> usize {
    let (mut i, mut n) = (from, 0);
    while n < LINE_BATCH && i + 64 <= b.len() {
        let mut m = newlines64(b, i);
        while m != 0 {
            out[n] = i + m.trailing_zeros() as usize + 1;
            n += 1;
            m &= m - 1;
        }
        i += 64;
    }
    if n >= LINE_BATCH { return n; }
    for (k, _) in b[i..].iter().enumerate().filter(|&(_, &c)| c == b'\n') {
        out[n] = i + k + 1;
        n += 1;
    }
    if (if n == 0 { from } else { out[n - 1] }) < b.len() {   // unterminated last line
        out[n] = b.len();
        n += 1;
    }
    n
}

#[cfg(test)]
fn masks(b: &[u8], p: usize) -> Masks {
    if p + 32 <= b.len() {
        masks32(&b[p..p + 32])
    } else {
        let mut buf = [0; 32];
        buf[..b.len() - p].copy_from_slice(&b[p..]);
        masks32(&buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::scan::{is_int, is_word};

    #[test]
    fn classes_match_predicates() {
        for c in 0..=255u8 {
            let class = LO[(c & 0xF) as usize] & HI[(c >> 4) as usize];
            let ws = matches!(c, b' ' | b'\t' | b'\r' | 0x0B | 0x0C);
            assert_eq!(class & WS != 0, ws, "ws {c:#x}");
            assert_eq!(class & TOKEN != 0, is_word(c) || is_int(c), "tok {c:#x}");
            assert_eq!(class & PUNCT != 0, matches!(c, b',' | b'(' | b')'), "punct {c:#x}");
        }
    }

    #[test]
    fn masks_per_byte() {
        // every byte value at every lane position, against the table
        for shift in 0..32 {
            let w: Vec<u8> = (0..=255u8).cycle().skip(shift).take(32 * 8).collect();
            for p in (0..w.len() - 32).step_by(7) {
                let m = masks(&w, p);
                for k in 0..32 {
                    let c = w[p + k];
                    let class = LO[(c & 0xF) as usize] & HI[(c >> 4) as usize];
                    assert_eq!(m.ws >> k & 1 == 1, class & WS != 0);
                    assert_eq!(m.tok >> k & 1 == 1, class & TOKEN != 0);
                    assert_eq!(m.punct >> k & 1 == 1, class & PUNCT != 0 || c == b':');
                }
            }
        }
        // short tail is zero-padded
        let m = masks(b"ab", 0);
        assert_eq!((m.tok, m.ws, m.punct), (0b11, 0, 0));
    }

    // every line start, batch by batch, the way assemble_part calls it
    fn all_ends(b: &[u8]) -> Vec<usize> {
        let mut ends = [0; LINE_BATCH + 64];
        let (mut all, mut pos) = (Vec::new(), 0);
        while pos < b.len() {
            let n = line_ends(b, pos, &mut ends);
            all.extend_from_slice(&ends[..n]);
            pos = ends[n - 1];
        }
        all
    }

    #[test]
    fn line_starts() {
        assert_eq!(all_ends(b"ab\n\ncd\n0123456789\nlast"), [3, 4, 7, 18, 22]);
        assert_eq!(all_ends(b"\n"), [1]);

        // sparse, dense (more than 8 per block, full blocks of '\n'), and past one batch
        for every in [7, 3, 1] {
            let src: Vec<u8> = (0..3000).map(|k| if k % every == 0 || k % 64 == 63 { b'\n' } else { b'a' }).collect();
            let mut want: Vec<usize> = (0..3000).filter(|k| src[*k] == b'\n').map(|k| k + 1).collect();
            if want.last() != Some(&3000) {
                want.push(3000);
            }
            assert_eq!(all_ends(&src), want, "every {every}");
        }
    }
}
