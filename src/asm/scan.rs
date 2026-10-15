use super::token::Token;

const ERR: &str = "scanning error";

// SIMD Within A Register (SWAR) helpers
const ONES: u64 = 0x0101_0101_0101_0101;
const HIGHS: u64 = 0x8080_8080_8080_8080;

#[inline(always)]
pub(super) fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

#[inline(always)]
pub(super) fn is_int(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'+'
}

#[inline(always)]
pub(super) fn munch(b: &[u8], mut i: usize, pred: fn(u8) -> bool) -> usize {
    while i < b.len() && pred(b[i]) { i += 1; }
    i
}

#[inline(always)]
pub(super) fn load(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i+8].try_into().unwrap())
}

#[inline(always)]
pub(super) fn reg_of(k: u64) -> Option<u32> {
    let (tk, n) = REGS[slot(k, REG_MAGIC_HASH, 7)];
    (tk == k).then_some(n)
}

#[inline(always)]
pub(super) fn key_at(b: &[u8], s: usize, e: usize) -> Option<u64> {
    let n = e - s;
    if n > 8 {
        return None;
    }

    if s + 8 <= b.len() {
        let w = load(b, s);
        return Some(
            if n == 8 {
                w
            }
            else {
                w & ((1 << (8 * n)) - 1)
            }
        );
    }

    let mut buf = [0; 8];
    buf[..n].copy_from_slice(&b[s..e]);

    Some(u64::from_le_bytes(buf))
}

pub const fn key(s: &[u8]) -> u64 {
    let mut k = 0;
    let mut i = 0;
    while i < s.len() {
        k |= (s[i] as u64) << (8 * i); i += 1;
    }
    k
}

pub fn word_key(s: &str) -> Option<u64> {
    key_at(s.as_bytes(), 0, s.len())
}

pub const fn slot(k: u64, m: u64, bits: u32) -> usize {
    // Implementation of the multiplicative hash function
    (k.wrapping_mul(m) >> (64 - bits)) as usize
}

const REG_NAMES: [(&str, u32); 65] = [
    // Numeric names (x0 - x31)
    ("x0", 0),
    ("x1", 1),
    ("x2", 2),
    ("x3", 3),
    ("x4", 4),
    ("x5", 5),
    ("x6", 6),
    ("x7", 7),
    ("x8", 8),
    ("x9", 9),
    ("x10", 10),
    ("x11", 11),
    ("x12", 12),
    ("x13", 13),
    ("x14", 14),
    ("x15", 15),
    ("x16", 16),
    ("x17", 17),
    ("x18", 18),
    ("x19", 19),
    ("x20", 20),
    ("x21", 21),
    ("x22", 22),
    ("x23", 23),
    ("x24", 24),
    ("x25", 25),
    ("x26", 26),
    ("x27", 27),
    ("x28", 28),
    ("x29", 29),
    ("x30", 30),
    ("x31", 31),

    // ABI Aliases
    ("zero", 0),
    ("ra", 1),
    ("sp", 2),
    ("gp", 3),
    ("tp", 4),
    ("t0", 5),
    ("t1", 6),
    ("t2", 7),
    ("s0", 8),
    ("fp", 8), // fp is an alias for s0 (x8)
    ("s1", 9),
    ("a0", 10),
    ("a1", 11),
    ("a2", 12),
    ("a3", 13),
    ("a4", 14),
    ("a5", 15), 
    ("a6", 16), 
    ("a7", 17),
    ("s2", 18), 
    ("s3", 19),
    ("s4", 20),
    ("s5", 21),
    ("s6", 22),
    ("s7", 23),
    ("s8", 24),
    ("s9", 25),
    ("s10", 26),
    ("s11", 27),
    ("t3", 28),
    ("t4", 29),
    ("t5", 30),
    ("t6", 31),
];
const REG_MAGIC_HASH: u64 = 0xa5f2a3e0788cbe85;

// "we have hash map at home"
// hashmap at home:
static REGS: [(u64, u32); 128] = {
    let mut t = [(0, 0); 128];
    let mut i = 0;
    while i < REG_NAMES.len() {
        let r = key(REG_NAMES[i].0.as_bytes());
        let h = slot(r, REG_MAGIC_HASH, 7);

        // This shouldn't happen...
        assert!(t[h].0 == 0, "register hash collision");

        t[h] = (r, REG_NAMES[i].1);
        i += 1;
    }

    t
};

pub(super) fn skip_line(b: &[u8], mut i: usize) -> usize {
    while i + 8 < b.len() {
        let x = load(b, i) ^ (ONES * b'\n' as u64);
        let z = x.wrapping_sub(ONES) & !x & HIGHS; // top 10 bitwise tricks oat

        if z != 0 {
            return i + (z.trailing_zeros() / 8) as usize + 1;
        }
        i += 8;
    }

    b[i..].iter().position(
        |&c| c == b'\n'
    ).map_or(
        b.len(), 
        |n| i + n + 1
    )
}

#[inline(always)]
pub(super) fn skip_ws(b: &[u8], mut i: usize) -> usize {
    loop {
        if i + 8 <= b.len() {
            let z = load(b, i) ^ (ONES * b' ' as u64);

            // All 8 bytes were spaces (indentation)
            if z == 0 {
                i += 8;
                continue;
            }

            i += (z.trailing_zeros() / 8) as usize;
        }
        match b.get(i) {
            Some(b' ' | b'\t' | b'\r' | 0x0B | 0x0C) => i += 1,
            _ => return i
        }
    }
}

#[cfg(test)]
pub fn tokenize(src: &str) -> Result<Vec<Token<'_>>, String> {
    // scan_line handles '\n' and '#' itself and returns where the next line starts

    let mut tokens = Vec::new();
    let (mut pos, mut line) = (0, 0);
    while pos < src.len() {
        line += 1;
        pos = scan_line(src, pos, &mut tokens).map_err(
            |e| format!("line {line}: {e}")
        )?;
        tokens.push(Token::Newline);
    }
    Ok(tokens)
}

pub(super) fn scan_line<'a>(src: &'a str, mut i: usize, out: &mut Vec<Token<'a>>) -> Result<usize, String> {
    // RV32I MM "DFA"

    // The actual DFA is super simple and does not specify tokens
    // We differentiate tokens in the actual code

    let b = src.as_bytes();

    while i < b.len() {
        match b[i] {
            b'\n' => return Ok(i + 1),
            b' ' | b'\t' | b'\r' | 0x0B | 0x0C => i += 1,
            b'#' => return Ok(skip_line(b, i)),
            // punctuation
            b',' => {
                out.push(Token::Comma); i += 1;
            }
            b'(' => {
                out.push(Token::LParen); i += 1;
            }
            b')' => {
                out.push(Token::RParen); i += 1;
            }
            // numerical literals (decimal or hex) with signing
            b'0'..=b'9' | b'-' | b'+' => {
                let end = munch(b, i, is_int);
                out.push(Token::Int(parse_int(&src[i..end])?));
                i = end;
            }
            // words, ids, registers, labels
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let end = munch(b, i, is_word);
                let lexeme = &src[i..end];
                if b.get(end) == Some(&b':') {
                    if reg_number(lexeme).is_some() {
                        return Err(format!("{ERR}: invalid label name '{lexeme}'"));
                    }
                    out.push(Token::Label(lexeme));
                    i = end + 1;
                } else {
                    out.push(match reg_number(lexeme) {
                        Some(reg) => Token::Reg(reg),
                        None => Token::Id(lexeme),
                    });
                    i = end;
                }
            }
            _ => {   // non-ASCII: only Unicode whitespace is allowed
                let c = src[i..].chars().next().unwrap();
                if !c.is_whitespace() {
                    return Err(format!("{ERR}: unexpected character '{c}'"));
                }
                i += c.len_utf8();
            }
        }
    }

    Ok(i)
}

pub(super) fn parse_int(s: &str) -> Result<i64, String> {
    // decimal (42, -8) and hex (0x1F, -0x10) in full radix complement
    let err = || format!("{ERR}: invalid integer '{s}'");

    let (neg_sign, rem) = match s.as_bytes() {
        [b'-', r @ ..] => (true, r),
        [b'+', r @ ..] => (false, r),
        r => (false, r),
    };
    let (rem, radix) = match rem {
        [b'0', b'x' | b'X', r @ ..] => (r, 16),
        r => (r, 10),
    };

    if rem.is_empty() || (radix == 10 && rem.len() > 1 && rem[0] == b'0') {
        return Err(err());
    }

    let mut n: i64 = 0;
    for &c in rem {
        let d = (c as char).to_digit(
            radix
        ).ok_or_else(err)? as i64;
        
        n = n.checked_mul(
            radix as i64
        ).and_then(
            |n| n.checked_add(d)
        ).ok_or_else(err)?;
    }
    Ok(if neg_sign { -n } else { n })
}

fn reg_number(name: &str) -> Option<u32> {
    reg_of(word_key(name)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints() {
        assert_eq!(parse_int("42"), Ok(42));
        assert_eq!(parse_int("-8"), Ok(-8));
        assert_eq!(parse_int("0x1F"), Ok(31));
        assert_eq!(parse_int("-0x10"), Ok(-16));
        assert!(parse_int("12ab").is_err());
        assert!(parse_int("0x").is_err());
        assert!(parse_int("--5").is_err());
        assert!(parse_int("+-5").is_err());
        assert!(parse_int("0x-5").is_err());
        assert!(parse_int("010").is_err());
        assert!(parse_int("-010").is_err());
        assert!(parse_int("00").is_err());
        assert_eq!(parse_int("0"), Ok(0));
        assert_eq!(parse_int("-0"), Ok(0));
        assert_eq!(parse_int("0x0010"), Ok(16));
        assert!(parse_int("--0x67").is_err());
        assert!(parse_int("0-x5").is_err());
        // just past i64 and u64: must not wrap around to a small value
        assert_eq!(parse_int("9223372036854775807"), Ok(i64::MAX));
        assert!(parse_int("9223372036854775808").is_err());
        assert!(parse_int("18446744073709551616").is_err());
        assert!(parse_int("0x10000000000000000").is_err());
        assert!(parse_int("-").is_err());
        assert!(parse_int("+").is_err());
    }

    #[test]
    fn registers() {
        let cases = [
            ("x0", 0), ("x31", 31), ("zero", 0), ("ra", 1), ("sp", 2), ("gp", 3), ("tp", 4),
            ("t0", 5), ("t2", 7), ("t3", 28), ("t6", 31), ("s0", 8), ("fp", 8), ("s1", 9),
            ("s2", 18), ("s11", 27), ("a0", 10), ("a7", 17),
        ];
        for (name, n) in cases {
            assert_eq!(reg_number(name), Some(n), "{name}");
        }
        for bad in ["x32", "t7", "s12", "a8", "add", "sw"] {
            assert_eq!(reg_number(bad), None, "{bad}");
        }
    }

    #[test]
    fn full_line() {
        let got = tokenize("loop: lw t0, -4(sp)   # comment\n").unwrap();
        let want = vec![
            Token::Label("loop"),
            Token::Id("lw"),
            Token::Reg(5),
            Token::Comma,
            Token::Int(-4),
            Token::LParen,
            Token::Reg(2),
            Token::RParen,
            Token::Newline,
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn lone_label() {
        let want = vec![Token::Label("loop"), Token::Newline];
        assert_eq!(tokenize("loop:\n").unwrap(), want);
    }

    #[test]
    fn no_spaces() {
        let got = tokenize("beq x1,x2,-8").unwrap();
        let want = vec![
            Token::Id("beq"),
            Token::Reg(1),
            Token::Comma,
            Token::Reg(2),
            Token::Comma,
            Token::Int(-8),
            Token::Newline,
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn hyphen_in_word() {
        assert!(tokenize("a-b").is_err());
    }

    #[test]
    fn non_ascii_error() {
        assert!(tokenize("addé x1, x2, x3").is_err());
    }

    #[test]
    fn register_label() {
        assert!(tokenize("t0:").is_err());
        assert!(tokenize("x31:").is_err());
        assert!(tokenize("x32:").is_ok());
    }

    #[test]
    fn error_line_number() {
        let e = tokenize("add x1, x2, x3\n\n# c\nadd x1, x2, $3\n").unwrap_err();
        assert!(e.contains("line 4"), "{e}");
    }

    #[test]
    fn blank_and_comments() {
        let want = vec![Token::Newline, Token::Newline, Token::Newline];
        assert_eq!(tokenize("\n   \n# only a comment\n").unwrap(), want);
    }

    #[test]
    fn next_line_start() {
        // scan_line returns where the next line begins; a comment runs to the '\n'
        let src = "add x1, x2, x3 # c\nsub x1, x2, x3";
        let mut toks = Vec::new();
        assert_eq!(scan_line(src, 0, &mut toks), Ok(19));
        assert_eq!(scan_line(src, 19, &mut toks), Ok(src.len()));
        assert_eq!(toks.len(), 12);
    }

    #[test]
    fn crlf_and_unicode_space() {
        // \r is whitespace; non-ASCII whitespace is still skipped
        assert_eq!(tokenize("ecall\r\nebreak\r\n").unwrap(), vec![
            Token::Id("ecall"), Token::Newline, Token::Id("ebreak"), Token::Newline,
        ]);
        assert_eq!(tokenize("ecall\u{a0}\n").unwrap(), vec![Token::Id("ecall"), Token::Newline]);
    }

    #[test]
    fn swar_skips() {
        let b = b"#                   comment\nnext";
        assert_eq!(skip_line(b, 0), 28);
        assert_eq!(skip_line(b"# no newline", 0), 12);
        assert_eq!(skip_line(b"\n", 0), 1);
        assert_eq!(skip_line(b"1234567\n", 0), 8);
        assert_eq!(skip_ws(b"                 \t x", 0), 19);
        assert_eq!(skip_ws(b"   ", 0), 3);
        assert_eq!(skip_ws(b"\n", 0), 0)
    }

    #[test]
    fn garbage_error() {
        assert!(tokenize("add x1, x2, $3").is_err());
    }
}
