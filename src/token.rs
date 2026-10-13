use std::fmt;
use std::str::FromStr;

const ERR: &str = "tokenization error";

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Label(String),
    Id(String),
    Reg(u32),
    Int(i64),
    Comma,
    LParen,
    RParen,
    Newline,
}

fn no_lexeme(tlex: &str, token_name: &str) -> Result<(), String> {
    if !tlex.is_empty() {
        return Err(format!("{ERR}: Token {token_name} unexpected extra data: '{tlex}'"));
    }
    Ok(())
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Token::Label(name) => write!(f, "LABEL {name}"),
            Token::Id(name) => write!(f, "ID {name}"),
            Token::Reg(n) => write!(f, "REG {n}"),
            Token::Int(n) => write!(f, "INT {n}"),
            Token::Comma => write!(f, "COMMA"),
            Token::LParen => write!(f, "LPAREN"),
            Token::RParen => write!(f, "RPAREN"),
            Token::Newline => write!(f, "NEWLINE"),
        }
    }
}

impl FromStr for Token {
    type Err = String;

    fn from_str(line: &str) -> Result<Token, String> {
        // the inverse of Display
        // - "REG 5" -> Token::Reg(5), "COMMA" -> Token::Comma
        // - anything malformed (unknown kind, missing or unparsable lexeme) -> Err(message)

        let (ttype, tlex) = line.split_once(' ').unwrap_or((line, ""));

        match ttype {
            "LABEL" => {
                if tlex.is_empty() {
                    return Err(format!("{ERR}: missing label name"));
                }
                Ok(Token::Label(tlex.to_string()))
            }
            "ID" => {
                if tlex.is_empty() {
                    return Err(format!("{ERR}: missing ID name"));
                }
                Ok(Token::Id(tlex.to_string()))
            }
            "REG" => {
                let n = tlex
                    .parse::<u32>()
                    .map_err(|_| format!("{ERR}: invalid register number '{tlex}'"))?;
                if n > 31 {
                    return Err(format!("{ERR}: register number {n} out of range"));
                }
                Ok(Token::Reg(n))
            }
            "INT" => {
                let n = tlex
                    .parse::<i64>()
                    .map_err(|_| format!("{ERR}: invalid integer '{tlex}'"))?;
                Ok(Token::Int(n))
            }
            "COMMA" => {
                no_lexeme(tlex, "COMMA")?;
                Ok(Token::Comma)
            }
            "LPAREN" => {
                no_lexeme(tlex, "LPAREN")?;
                Ok(Token::LParen)
            }
            "RPAREN" => {
                no_lexeme(tlex, "RPAREN")?;
                Ok(Token::RParen)
            }
            "NEWLINE" => {
                no_lexeme(tlex, "NEWLINE")?;
                Ok(Token::Newline)
            }
            other => Err(format!("{ERR}: unknown token kind '{other}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let all = [
            Token::Label("loop".to_string()),
            Token::Id("addi".to_string()),
            Token::Reg(31),
            Token::Int(-8),
            Token::Comma,
            Token::LParen,
            Token::RParen,
            Token::Newline,
        ];
        for t in all {
            assert_eq!(t.to_string().parse::<Token>(), Ok(t));
        }
    }

    #[test]
    fn extreme_values() {
        for t in [Token::Reg(0), Token::Reg(31), Token::Int(i64::MIN), Token::Int(i64::MAX), Token::Int(0)] {
            assert_eq!(t.to_string().parse::<Token>(), Ok(t));
        }
    }

    #[test]
    fn malformed() {
        let bad = [
            "",
            "BOGUS 1",
            "reg 5",
            "LABEL",
            "ID",
            "ID ",
            "REG",
            "REG x5",
            "REG -1",
            "REG 32",
            "INT",
            "INT 0x10",
            "INT 1.5",
            "INT 99999999999999999999",
            "COMMA ,",
            "NEWLINE x",
        ];
        for line in bad {
            assert!(line.parse::<Token>().is_err(), "{line:?} should be rejected");
        }
    }

    #[test]
    fn error_prefix() {
        let e = "BOGUS".parse::<Token>().unwrap_err();
        assert!(e.starts_with("tokenization error: "), "{e}");
    }
}
