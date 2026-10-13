use landfill::token::Token;
use std::io::{self, Read};

const ERR: &str = "scanning error";

fn main() {
    if let Err(e) = run() {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut src = String::new();
    io::stdin().read_to_string(&mut src).map_err(|e| e.to_string())?;

    for t in tokenize(&src)? {
        println!("{t}");
    }
    Ok(())
}

fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    for line in src.lines() {
        let code = line.split('#').next().unwrap_or("");
        let before = tokens.len();
        scan_line(code, &mut tokens)?;
        if tokens.len() > before {
            tokens.push(Token::Newline);
        }
    }
    Ok(tokens)
}

fn munch(s: &str, pred: impl Fn(char) -> bool) -> (&str, &str) {
    let end = s.find(|c: char| !pred(c)).unwrap_or(s.len());
    s.split_at(end)
}

fn scan_line(rest: &str, out: &mut Vec<Token>) -> Result<(), String> {
    // TODO: loop until `rest` is empty. Each iteration: skip whitespace, look at the first char,
    // munch the longest token starting there, push it to `out`, continue with what's left.
    // Words become Label (if followed by ':'), Reg (if reg_number knows them), or Id.
    // Anything unrecognized -> Err(format!("{ERR}: ...")).
    todo!()
}

fn parse_int(s: &str) -> Result<i64, String> {
    // TODO: decimal ("42", "-8") and hex ("0x1F", "-0x10").
    todo!()
}

fn reg_number(name: &str) -> Option<u32> {
    // TODO: x0..x31 and the ABI names (zero ra sp gp tp t0-t6 s0-s11 fp a0-a7).
    todo!()
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
    fn blank_and_comment_lines_vanish() {
        assert_eq!(tokenize("\n   \n# only a comment\n").unwrap(), vec![]);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(tokenize("add x1, x2, $3").is_err());
    }
}
