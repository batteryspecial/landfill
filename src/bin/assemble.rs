use landfill::encode;
use landfill::token::Token;
use std::{fmt::format, io::{self, Read, Write}};

const ERR: &str = "assembly error";

// Symbol Table
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
    // Walk the lines with a pc. Record each label at the current pc (error on duplicates).
    // Labels may only appear before the instruction on a line. Every instruction is 4 bytes.

    let mut symbols = Symbols::new();
    let mut pc: u32 = 0;

    for (i, l) in lines.iter().enumerate() {
        // 1. Split leading labels from the instruction
        let n_labels = l.iter().take_while(
            |t| matches!(t, Token::Label(_))
        ).count();
        let instr = &l[n_labels..];

        // 2. Ensure no labels appear after the instruction
        if instr.iter().any(|t| matches!(t, Token::Label(_))) {
            return Err(format!("line {}: {ERR}: label after instruction", i + 1));
        }

        // 3. Record every label at the current pc
        for t in &l[..n_labels] {
            if let Token::Label(name) = t {
                if symbols.iter().any(|(existing, _)| existing == name) {
                    return Err(format!("line {}: {ERR}: duplicate label '{name}'", i + 1));
                }
                symbols.push((name.clone(), pc));
            }
        }

        // 4. Advance if line contains an instruction
        if !instr.is_empty() {
            pc += 4;
        }
    }

    Ok(symbols)
}

fn pass2(lines: &[Vec<Token>], symbols: &Symbols) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut pc: u32 = 0;

    for line in lines {
        let n_labels = line.iter().take_while(
            |t| matches!(t, Token::Label(_))
        ).count();

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

#[derive(Clone, Copy)]
enum Op {
    R { funct3: u32, funct7: u32 },
    I { funct3: u32 },
    Shift { funct3: u32, funct7: u32 },
    Load { funct3: u32 },
    Branch { funct3: u32 },
    Fixed(u32),
    Store { funct3: u32 },
    Jal,
    Jalr,
    U { opcode: u32 },
}

fn lookup(mnemonic: &str) -> Option<Op> {
    use Op::*;
    Some(match mnemonic {
        "add" => R { funct3: 0x0, funct7: 0x00 },
        "sub" => R { funct3: 0x0, funct7: 0x20 },
        "xor" => R { funct3: 0x4, funct7: 0x00 },
        "or" => R { funct3: 0x6, funct7: 0x00 },
        "and" => R { funct3: 0x7, funct7: 0x00 },
        "sll" => R { funct3: 0x1, funct7: 0x00 },
        "srl" => R { funct3: 0x5, funct7: 0x00 },
        "sra" => R { funct3: 0x5, funct7: 0x20 },
        "slt" => R { funct3: 0x2, funct7: 0x00 },
        "sltu" => R { funct3: 0x3, funct7: 0x00 },

        "addi" => I { funct3: 0x0 },
        // TODO: xori ori andi slti sltiu

        "slli" => Shift { funct3: 0x1, funct7: 0x00 },
        "srai" => Shift { funct3: 0x5, funct7: 0x20 },
        // TODO: srli

        "lw" => Load { funct3: 0x2 },
        // TODO: lb lh lbu lhu

        "beq" => Branch { funct3: 0x0 },
        // TODO: bne blt bge bltu bgeu

        "ecall" => Fixed(0x00000073),
        // TODO: ebreak

        // TODO: sb sh sw, jal, jalr, lui, auipc
        _ => return None,
    })
}

fn target_offset(target: &Token, pc: u32, symbols: &Symbols) -> Result<i64, String> {
    match target {
        Token::Int(n) => Ok(*n),
        Token::Id(name) => symbols
            .iter()
            .find(|(label, _)| label == name)
            .map(|(_, addr)| *addr as i64 - pc as i64)
            .ok_or_else(|| format!("{ERR}: undefined label '{name}'")),
        other => Err(format!("{ERR}: expected label or offset, found {other}")),
    }
}

fn encode_line(instr: &[Token], pc: u32, symbols: &Symbols) -> Result<u32, String> {
    use Token::{Comma, Int, LParen, RParen, Reg};

    let Token::Id(mnemonic) = &instr[0] else {
        return Err(format!("{ERR}: expected instruction, found {}", instr[0]));
    };
    let op = lookup(mnemonic).ok_or_else(|| format!("{ERR}: unknown instruction '{mnemonic}'"))?;

    match (op, &instr[1..]) {
        (Op::R { funct3, funct7 }, [Reg(rd), Comma, Reg(rs1), Comma, Reg(rs2)]) => {
            Ok(encode::r_type(funct7, *rs2, *rs1, funct3, *rd, 0b0110011))
        }
        (Op::I { funct3 }, [Reg(rd), Comma, Reg(rs1), Comma, Int(imm)]) => {
            encode::i_type(*imm, *rs1, funct3, *rd, 0b0010011)
        }
        (Op::Shift { funct3, funct7 }, [Reg(rd), Comma, Reg(rs1), Comma, Int(shamt)]) => {
            if !(0..=31).contains(shamt) {
                return Err(format!("{ERR}: shift amount {shamt} out of range"));
            }
            encode::i_type((funct7 << 5) as i64 | *shamt, *rs1, funct3, *rd, 0b0010011)
        }
        (Op::Load { funct3 }, [Reg(rd), Comma, Int(imm), LParen, Reg(rs1), RParen]) => {
            encode::i_type(*imm, *rs1, funct3, *rd, 0b0000011)
        }
        (Op::Branch { funct3 }, [Reg(rs1), Comma, Reg(rs2), Comma, target]) => {
            let offset = target_offset(target, pc, symbols)?;
            encode::b_type(offset, *rs2, *rs1, funct3)
        }
        // TODO: arms for Store, Jal, Jalr, U
        (Op::Fixed(word), []) => Ok(word),
        _ => Err(format!("{ERR}: invalid operands for '{mnemonic}'")),
    }
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
            vec![],
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
        let lines = vec![vec![label("a"), label("a"), id("ecall")]];
        assert!(pass1(&lines).is_err());
    }

    #[test]
    fn pass1_label_after_instr() {
        let lines = vec![vec![], vec![id("ecall"), label("a")]];
        let e = pass1(&lines).unwrap_err();
        assert!(e.starts_with("line 2: assembly error: "), "{e}");
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
