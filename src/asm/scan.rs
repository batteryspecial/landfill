use super::token::Token;

const ERR: &str = "scanning error";

pub fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let code = line.split('#').next().unwrap_or("");
        scan_line(code, &mut tokens).map_err(
            |e| format!("line {}: {e}", i + 1)
        )?;
        tokens.push(Token::Newline);
    }
    Ok(tokens)
}

fn munch(s: &str, pred: impl Fn(char) -> bool) -> (&str, &str) {
    let end = s.find(|c: char| !pred(c)).unwrap_or(s.len());
    s.split_at(end)
}

fn scan_line(mut rest: &str, out: &mut Vec<Token>) -> Result<(), String> {
    // RV32I MM "DFA"

    // The actual DFA is super simple and does not specify tokens
    // We differentiate tokens in the actual code

    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }

        let first = rest.chars().next().unwrap();
        match first {
            // punctuation
            ',' => {
                out.push(Token::Comma);
                rest = &rest[1..];
            }
            '(' => {
                out.push(Token::LParen);
                rest = &rest[1..];
            }
            ')' => {
                out.push(Token::RParen);
                rest = &rest[1..];
            }
            // numerical literals (decimal or hex) with signing
            '0'..='9' | '-' | '+' => {
                let (lexeme, rem) = munch(rest, |c| c.is_ascii_alphanumeric() || c == '-' || c == '+');
                let n = parse_int(lexeme)?;
                out.push(Token::Int(n));
                rest = rem;
            }
            // words, ids, registers, labels
            c if c.is_ascii_alphabetic() || c == '_' => {
                let (lexeme, rem) = munch(
                    rest, 
                    |c| c.is_ascii_alphanumeric() || c == '_'
                );

                if rem.starts_with(':') {
                    if reg_number(lexeme).is_some() {
                        return Err(format!("{ERR}: invalid label name '{lexeme}'"));
                    }
                    out.push(Token::Label(lexeme.to_string()));
                    rest = &rem[1..];
                } else {
                    if let Some(reg) = reg_number(lexeme) {
                        out.push(Token::Reg(reg));
                    } else {
                        out.push(Token::Id(lexeme.to_string()));
                    }

                    rest = rem;
                }
            }
            _ => {
                return Err(format!("{ERR}: unexpected character '{first}'"));
            }
        }
    }

    Ok(())
}

fn parse_int(s: &str) -> Result<i64, String> {
    // decimal (42, -8) and hex (0x1F, -0x10) in full radix complement
    
    let (neg_sign, rem) = if let Some(r) = s.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (false, r)
    } else {
        (false, s)
    };

    let (rem, radix) = if let Some(r) = rem.strip_prefix("0x").or_else(|| rem.strip_prefix("0X")) {
        (r, 16)
    } else {
        (rem, 10)
    };

    if rem.starts_with("+") || rem.starts_with("-") {
        return Err(format!("{ERR}: invalid integer '{s}'"));
    }
    if radix == 10 && rem.len() > 1 && rem.starts_with('0') {
        return Err(format!("{ERR}: invalid integer '{s}'"));
    }

    let rem = i64::from_str_radix(rem, radix);

    let n = rem.map_err(|_| format!("{ERR}: invalid integer '{s}'"))?;
    Ok(if neg_sign { -n } else { n })
}

fn reg_number(name: &str) -> Option<u32> {
    // x0..x31 and ABI names (zero ra sp gp tp t0-t6 s0-s11 fp a0-a7)

    if name.starts_with('x') {
        let digits = &name[1..];
        if digits.starts_with('0') && digits.len() > 1 {
            return None;
        }
        if let Ok(n) = digits.parse::<u32>() {
            if n < 32 {
                return Some(n);
            }
        }
    }

    match name {
        "zero" => Some(0),
        "ra" => Some(1),
        "sp" => Some(2),
        "gp" => Some(3),
        "tp" => Some(4),
        "t0" => Some(5),
        "t1" => Some(6),
        "t2" => Some(7),
        "s0" | "fp" => Some(8),
        "s1" => Some(9),
        "a0" => Some(10),
        "a1" => Some(11),
        "a2" => Some(12),
        "a3" => Some(13),
        "a4" => Some(14),
        "a5" => Some(15),
        "a6" => Some(16),
        "a7" => Some(17),
        "s2" => Some(18),
        "s3" => Some(19),
        "s4" => Some(20),
        "s5" => Some(21),
        "s6" => Some(22),
        "s7" => Some(23),
        "s8" => Some(24),
        "s9" => Some(25),
        "s10" => Some(26),
        "s11" => Some(27),
        "t3" => Some(28),
        "t4" => Some(29),
        "t5" => Some(30),
        "t6" => Some(31),
        _ => None,
    }
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
            Token::Label("loop".to_string()),
            Token::Id("lw".to_string()),
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
        let want = vec![Token::Label("loop".to_string()), Token::Newline];
        assert_eq!(tokenize("loop:\n").unwrap(), want);
    }

    #[test]
    fn no_spaces() {
        let got = tokenize("beq x1,x2,-8").unwrap();
        let want = vec![
            Token::Id("beq".to_string()),
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
    fn garbage_error() {
        assert!(tokenize("add x1, x2, $3").is_err());
    }
}
