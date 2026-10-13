use landfill::encode;
use landfill::token::Token;
use std::io::{self, Read, Write};

const ERR: &str = "assembly error";

// swap in a HashMap if label count ever matters
type Symbols = Vec<(String, u32)>;

fn main() {
    if let Err(e) = run() {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).map_err(|e| e.to_string())?;

    let mut tokens = Vec::new();
    for line in input.lines() {
        tokens.push(line.parse::<Token>()?);
    }

    let lines: Vec<Vec<Token>> = tokens
        .split(|t| *t == Token::Newline)
        .filter(|l| !l.is_empty())
        .map(|l| l.to_vec())
        .collect();

    let symbols = pass1(&lines)?;
    let bytes = pass2(&lines, &symbols)?;

    io::stdout().write_all(&bytes).map_err(|e| e.to_string())?;
    for (name, addr) in &symbols {
        eprintln!("{name} {addr}");
    }
    Ok(())
}

fn pass1(lines: &[Vec<Token>]) -> Result<Symbols, String> {
    // TODO: walk the lines with a pc. Record each label at the current pc (error on duplicates).
    // Labels may only appear before the instruction on a line. Every instruction is 4 bytes.
    todo!()
}

fn pass2(lines: &[Vec<Token>], symbols: &Symbols) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut pc: u32 = 0;
    for line in lines {
        let n_labels = line.iter().take_while(|t| matches!(t, Token::Label(_))).count();
        let instr = &line[n_labels..];
        if instr.is_empty() {
            continue;
        }
        let word = encode_line(instr, pc, symbols)?;
        out.extend_from_slice(&word.to_le_bytes());
        pc += 4;
    }
    Ok(out)
}

fn encode_line(instr: &[Token], pc: u32, symbols: &Symbols) -> Result<u32, String> {
    // TODO: instr[0] is the mnemonic (Token::Id). Match it to its operand shape, check the
    // remaining tokens fit that shape, resolve label operands to `target - pc`, then call
    // the matching encode:: function.
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Token {
        Token::Id(s.to_string())
    }
    fn label(s: &str) -> Token {
        Token::Label(s.to_string())
    }
    use Token::{Comma, Int, LParen, RParen, Reg};

    #[test]
    fn pass1_addresses() {
        let lines = vec![
            vec![label("a")],
            vec![id("ecall")],
            vec![label("b"), label("c"), id("ecall")],
            vec![label("d")],
        ];
        let want = vec![
            ("a".to_string(), 0),
            ("b".to_string(), 4),
            ("c".to_string(), 4),
            ("d".to_string(), 8),
        ];
        assert_eq!(pass1(&lines), Ok(want));
    }

    #[test]
    fn pass1_duplicate() {
        let lines = vec![vec![label("a")], vec![label("a"), id("ecall")]];
        assert!(pass1(&lines).is_err());
    }

    #[test]
    fn pass1_label_after_instr() {
        let lines = vec![vec![id("ecall"), label("a")]];
        assert!(pass1(&lines).is_err());
    }

    #[test]
    fn r_shape() {
        let instr = [id("add"), Reg(3), Comma, Reg(1), Comma, Reg(2)];
        assert_eq!(encode_line(&instr, 0, &vec![]), Ok(0x002081B3));
    }

    #[test]
    fn load_shape() {
        let instr = [id("lw"), Reg(5), Comma, Int(-4), LParen, Reg(2), RParen];
        assert_eq!(encode_line(&instr, 0, &vec![]), Ok(0xFFC12283));
    }

    #[test]
    fn shift_imm() {
        let instr = [id("srai"), Reg(1), Comma, Reg(2), Comma, Int(3)];
        assert_eq!(encode_line(&instr, 0, &vec![]), Ok(0x40315093));
        let instr = [id("slli"), Reg(1), Comma, Reg(2), Comma, Int(32)];
        assert!(encode_line(&instr, 0, &vec![]).is_err());
    }

    #[test]
    fn branch_to_label() {
        let symbols = vec![("loop".to_string(), 0)];
        let instr = [id("beq"), Reg(1), Comma, Reg(2), Comma, id("loop")];
        assert_eq!(encode_line(&instr, 8, &symbols), Ok(0xFE208CE3));
    }

    #[test]
    fn no_operands() {
        assert_eq!(encode_line(&[id("ecall")], 0, &vec![]), Ok(0x00000073));
    }

    #[test]
    fn errors() {
        let undefined = [id("jal"), Reg(1), Comma, id("nowhere")];
        assert!(encode_line(&undefined, 0, &vec![]).is_err());
        let wrong_shape = [id("add"), Reg(1), Comma, Reg(2)];
        assert!(encode_line(&wrong_shape, 0, &vec![]).is_err());
        let unknown = [id("mul"), Reg(1), Comma, Reg(2), Comma, Reg(3)];
        assert!(encode_line(&unknown, 0, &vec![]).is_err());
    }
}
