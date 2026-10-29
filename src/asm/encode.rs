/**
 * encode.rs
 * 
 * Parse exactly the operand shape a mnemonic expects.
 *  - Token positions come from masks after the line's indentation.
 *  - Every token start is a bit, step is starts &= starts - 1
 */

use super::classify;
use super::scan::{is_word, parse_int, reg_of, skip_ws};
use super::{Fixup, Op, State, types, encode_jump, op_of};
use std::collections::hash_map::Entry;

// w is the 64 bytes from base plus 8 so any key load fits, zero past the end 
// Indexing it with offsets masked to the window needs no bounds checks.
struct Tokens<'a, 'w> {
    src: &'a str,
    w: &'w [u8; 72],
    base: usize,
    win: usize,
    starts: u64,
    tok: u64,
}

enum Target<'a> {
    Offset(i64),
    Forward(&'a str),
}

impl<'a> Tokens<'a, '_> {
    // Start of the next token, relative to base
    #[inline(always)]
    fn next(&mut self) -> Option<usize> {
        if self.starts == 0 {
            return None;
        }
        let s = self.starts.trailing_zeros() as usize;
        self.starts &= self.starts - 1;
        Some(s)
    }

    #[inline(always)]
    fn byte(&self, rel: usize) -> u8 {
        self.w[rel & 63]
    }

    // First byte of the next token, without taking it
    #[inline(always)]
    fn peek(&self) -> Option<u8> {
        (self.starts != 0).then(|| self.byte(self.starts.trailing_zeros() as usize))
    }

    // A word or integer: (start, end) relative to base
    #[inline(always)]
    fn run(&mut self) -> Option<(usize, usize)> {
        let s = self.next()?;
        let len = (!self.tok >> s).trailing_zeros() as usize;
        // len 0 is punctuation; a run reaching the window's end may continue past it
        (len != 0 && s + len < self.win).then_some((s, s + len))
    }

    #[inline(always)]
    // The word's bytes as a little-endian u64, if it fits in 8
    fn key(&self, s: usize, e: usize) -> Option<u64> {
        let n = e - s;
        if n > 8 {
            return None;
        }
        let s = s & 63;
        let x = u64::from_le_bytes(self.w[s..s + 8].try_into().unwrap());
        Some(x & (!0 >> (64 - 8 * n)))
    }

    #[inline(always)]
    fn reg(&mut self) -> Option<u32> {
        let (s, e) = self.run()?;
        reg_of(self.key(s, e)?)
    }

    #[inline(always)]
    fn punct(&mut self, c: u8) -> Option<()> {
        let s = self.next()?;
        (self.byte(s) == c).then_some(())
    }

    #[inline(always)]
    fn comma(&mut self) -> Option<()> {
        self.punct(b',')
    }

    #[inline(always)]
    fn int(&mut self) -> Option<i64> {
        let (s, e) = self.run()?;
        let c = self.byte(s);
        if !(c.is_ascii_digit() || c == b'-' || c == b'+') {
            return None;
        }
        parse_int(&self.src[self.base + s..self.base + e]).ok()
    }

    // A label name: a whole word that is not a register
    #[inline(always)]
    fn name(&self, s: usize, e: usize) -> Option<&'a str> {
        let name = &self.src[self.base + s..self.base + e];
        let c = name.as_bytes()[0];
        let word = (c.is_ascii_alphabetic() || c == b'_') && name.bytes().all(is_word);
        (word && self.key(s, e).and_then(reg_of).is_none()).then_some(name)
    }

    // imm(rs1) or (rs1)
    #[inline(always)]
    fn mem(&mut self) -> Option<(i64, u32)> {
        let imm = if self.peek()? == b'(' { 0 } else { self.int()? };
        self.punct(b'(')?;
        let rs1 = self.reg()?;
        self.punct(b')')?;
        Some((imm, rs1))
    }

    #[inline(always)]
    fn target(&mut self, st: &State<'a>) -> Option<Target<'a>> {
        let c = self.peek()?;
        if !(c.is_ascii_alphabetic() || c == b'_') {
            return self.int().map(Target::Offset);
        }
        let (s, e) = self.run()?;
        let name = self.name(s, e)?;
        Some(match st.index.get(name) {
            Some(&addr) => Target::Offset(addr as i64 - st.pc as i64),
            None => Target::Forward(name),
        })
    }
}

// Parse the line starting at start; false if the general path must redo it
// Inlined into the line loop so the classifier's constants stay in registers
#[inline(always)]
pub(super) fn line<'a>(src: &'a str, start: usize, line: usize, st: &mut State<'a>) -> bool {
    let b = src.as_bytes();
    let base = skip_ws(b, start);
    let end = |i: usize| i >= b.len() || matches!(b[i], b'\n' | b'#');

    let mut pad = [0; 72];
    let w: &[u8; 72] = match b.get(base..base + 72) {
        Some(w) => w.try_into().unwrap(),
        None => {
            pad[..b.len() - base].copy_from_slice(&b[base..]);
            &pad
        }
    };

    let m = classify::masks32(&w[..32]);
    let (mut ws, mut tok, mut punct) = (m.ws as u64, m.tok as u64, m.punct as u64);
    let mut win = 32;
    // Code ends at the first byte in no class: '\n', '#', end of input, or junk
    let mut lim = (!(m.ws | m.tok | m.punct)).trailing_zeros() as usize;
    if lim < 32 {
        if !end(base + lim) {
            return false;
        }
    } else if m.tok >> 31 == 1 || !end(skip_ws(b, base + 32)) {
        // a token may cross byte 32, or code continues past it: classify the next 32 too
        let m = classify::masks32(&w[32..64]);
        (ws, tok, punct) = (ws | (m.ws as u64) << 32, tok | (m.tok as u64) << 32, punct | (m.punct as u64) << 32);
        win = 64;
        lim = (!(ws | tok | punct)).trailing_zeros() as usize;
        let blank_after = if lim < 64 { end(base + lim) } else { end(skip_ws(b, base + 64)) };
        if !blank_after {
            return false; // junk, or more code past 64 bytes
        }
    }
    let code = if lim < 64 { (1u64 << lim) - 1 } else { !0 };
    let starts = (punct | tok & !(tok << 1)) & code;

    let n_labels = st.order.len();
    let ok = parse(&mut Tokens { src, w, base, win, starts, tok }, line, st).is_some();
    if !ok {
        // undo this line's labels before the general path redoes it
        for (name, _, _) in st.order.drain(n_labels..) {
            st.index.remove(name);
        }
    }
    ok
}

#[inline(always)]
fn parse<'a>(t: &mut Tokens<'a, '_>, line: usize, st: &mut State<'a>) -> Option<()> {
    // labels, then the mnemonic
    let op = loop {
        if t.starts == 0 {
            return Some(()); // blank, comment, or labels only
        }
        let (s, e) = t.run()?;
        if t.peek() != Some(b':') || t.starts.trailing_zeros() as usize != e {
            let c = t.byte(s);
            if !(c.is_ascii_alphabetic() || c == b'_') {
                return None;
            }
            break op_of(t.key(s, e)?)?;
        }
        t.next();
        let name = t.name(s, e)?;
        match st.index.entry(name) {
            Entry::Occupied(_) => return None,
            Entry::Vacant(v) => v.insert(st.pc),
        };
        st.order.push((name, st.pc, line));
    };

    let (word, target) = match op {
        Op::R { funct3, funct7 } => {
            let rd = t.reg()?;
            t.comma()?;
            let rs1 = t.reg()?;
            t.comma()?;
            let rs2 = t.reg()?;
            (types::r_type(funct7, rs2, rs1, funct3, rd, 0b0110011), None)
        }
        Op::I { funct3 } => {
            let rd = t.reg()?;
            t.comma()?;
            let rs1 = t.reg()?;
            t.comma()?;
            let imm = t.int()?;
            (types::i_type(imm, rs1, funct3, rd, 0b0010011).ok()?, None)
        }
        Op::Shift { funct3, funct7 } => {
            let rd = t.reg()?;
            t.comma()?;
            let rs1 = t.reg()?;
            t.comma()?;
            let shamt = t.int()?;
            if !(0..=31).contains(&shamt) {
                return None;
            }
            (types::i_type((funct7 << 5) as i64 | shamt, rs1, funct3, rd, 0b0010011).ok()?, None)
        }
        Op::Load { funct3 } => {
            let rd = t.reg()?;
            t.comma()?;
            let (imm, rs1) = t.mem()?;
            (types::i_type(imm, rs1, funct3, rd, 0b0000011).ok()?, None)
        }
        Op::Store { funct3 } => {
            let rs2 = t.reg()?;
            t.comma()?;
            let (imm, rs1) = t.mem()?;
            (types::s_type(imm, rs2, rs1, funct3).ok()?, None)
        }
        Op::Branch { .. } => {
            let rs1 = t.reg()?;
            t.comma()?;
            let rs2 = t.reg()?;
            t.comma()?;
            jump(op, t.target(st)?, rs1, rs2)?
        }
        Op::Jal => {
            let rd = t.reg()?;
            t.comma()?;
            jump(op, t.target(st)?, rd, 0)?
        }
        Op::Jalr => {
            let rd = t.reg()?;
            t.comma()?;
            let (imm, rs1) = if t.peek()?.is_ascii_alphabetic() {
                let rs1 = t.reg()?;
                t.comma()?;
                (t.int()?, rs1)
            } else {
                t.mem()?
            };
            (types::i_type(imm, rs1, 0x0, rd, 0b1100111).ok()?, None)
        }
        Op::U { opcode } => {
            let rd = t.reg()?;
            t.comma()?;
            let imm = t.int()?;
            (types::u_type(imm, rd, opcode).ok()?, None)
        }
        Op::Fixed(word) => (word, None),
    };
    if t.starts != 0 {
        return None; // trailing tokens
    }

    if let Some((label, rd_rs1, rs2)) = target {
        st.fixups.push(Fixup { line, pc: st.pc, label, op, rd_rs1, rs2 });
    }
    st.out.extend_from_slice(&word.to_le_bytes());
    st.pc += 4;
    Some(())
}

// Encoded word, plus the operands to patch once a forward label is known
fn jump<'a>(op: Op, t: Target<'a>, rd_rs1: u32, rs2: u32) -> Option<(u32, Option<(&'a str, u32, u32)>)> {
    match t {
        Target::Offset(offset) => Some((encode_jump(op, offset, rd_rs1, rs2).ok()?, None)),
        Target::Forward(label) => Some((0, Some((label, rd_rs1, rs2)))),
    }
}
