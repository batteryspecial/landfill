use landfill::encode;
use landfill::token::Token;
use std::{io::{self, Read, Write}};

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

    for (i, line) in lines.iter().enumerate() {
        let n_labels = line.iter().take_while(
            |t| matches!(t, Token::Label(_))
        ).count();

        let instr = &line[n_labels..];
        if instr.is_empty() {
            continue;
        }
        
        let word = encode_line(instr, pc, symbols).map_err(
            |e| format!("line {}: {e}", i + 1)
        )?;
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
        "xori" => I { funct3: 0x4 },
        "ori" => I { funct3: 0x6 },
        "andi" => I { funct3: 0x7 },
        "slti" => I { funct3: 0x2 },
        "sltiu" => I { funct3: 0x3 },

        "slli" => Shift { funct3: 0x1, funct7: 0x00 },
        "srai" => Shift { funct3: 0x5, funct7: 0x20 },
        "srli" => Shift { funct3: 0x5, funct7: 0x00 },

        "lw" => Load { funct3: 0x2 },
        "lb" => Load { funct3: 0x0 },
        "lh" => Load { funct3: 0x1 },
        "lbu" => Load { funct3: 0x4 },
        "lhu" => Load { funct3: 0x5 },
        
        "sb" => Store { funct3: 0x0 },
        "sh" => Store { funct3: 0x1 },
        "sw" => Store { funct3: 0x2 },

        "beq" => Branch { funct3: 0x0 },
        "bne" => Branch { funct3: 0x1 },
        "blt" => Branch { funct3: 0x4 },
        "bge" => Branch { funct3: 0x5 },
        "bltu" => Branch { funct3: 0x6 },
        "bgeu" => Branch { funct3: 0x7 },

        "jal" => Jal,
        "jalr" => Jalr,
        "lui" => U { opcode: 0x37 },
        "auipc" => U { opcode: 0x17 },

        "ecall" => Fixed(0x00000073),
        "ebreak" => Fixed(0x00100073),

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
        other => Err(format!("{ERR}: expected label or offset, received '{other}'")),
    }
}

fn mem_operand(ops: &[Token]) -> Result<(i64, u32), String> {
    match ops {
        [Token::Int(imm), Token::LParen, Token::Reg(rs1), Token::RParen] => Ok((*imm, *rs1)),
        [Token::LParen, Token::Reg(rs1), Token::RParen] => Ok((0, *rs1)),
        _ => Err(format!("{ERR}: expected offset(register)")),
    }
}

fn encode_line(instr: &[Token], pc: u32, symbols: &Symbols) -> Result<u32, String> {
    use Token::{Comma, Int, Reg};

    let Token::Id(mnemonic) = &instr[0] else {
        return Err(format!("{ERR}: expected instruction, received '{}'", instr[0]));
    };

    let op = lookup(mnemonic).ok_or_else(|| format!("{ERR}: unknown instruction '{mnemonic}'"))?;

    match (op, &instr[1..]) {
        (Op::R { funct3, funct7 }, 
            [Reg(rd), Comma, Reg(rs1), Comma, Reg(rs2)]
        ) => {
            Ok(encode::r_type(funct7, *rs2, *rs1, funct3, *rd, 0b0110011))
        }

        (Op::I { funct3 }, 
            [Reg(rd), Comma, Reg(rs1), Comma, Int(imm)]
        ) => {
            encode::i_type(*imm, *rs1, funct3, *rd, 0b0010011)
        }

        (Op::Shift { funct3, funct7 }, 
            [Reg(rd), Comma, Reg(rs1), Comma, Int(shamt)]
        ) => {
            if !(0..=31).contains(shamt) {
                return Err(format!("{ERR}: shift amount '{shamt}' out of range"));
            }
            encode::i_type((funct7 << 5) as i64 | *shamt, *rs1, funct3, *rd, 0b0010011)
        }

        (Op::Load { funct3 }, 
            [Reg(rd), Comma, mem @ ..]
        ) => {
            let (imm, rs1) = mem_operand(mem)?;
            encode::i_type(imm, rs1, funct3, *rd, 0b0000011)
        }
        
        (Op::Store { funct3 }, 
            [Reg(rs2), Comma, mem @ ..]
        ) => {
            let (imm, rs1) = mem_operand(mem)?;
            encode::s_type(imm, *rs2, rs1, funct3)
        }

        (Op::Branch { funct3 }, 
            [Reg(rs1), Comma, Reg(rs2), Comma, target]
        ) => {
            let offset = target_offset(target, pc, symbols)?;
            encode::b_type(offset, *rs2, *rs1, funct3)
        }

        (Op::Jal, 
            [Reg(rd), Comma, target]
        ) => {
            let offset = target_offset(target, pc, symbols)?;
            encode::j_type(offset, *rd)
        }

        (Op::Jalr,
            [Reg(rd), Comma, Reg(rs1), Comma, Int(imm)]
        ) => {
            encode::i_type(*imm, *rs1, 0x0, *rd, 0b1100111)
        }

        (Op::Jalr,
            [Reg(rd), Comma, mem @ ..]
        ) => {
            let (imm, rs1) = mem_operand(mem)?;
            encode::i_type(imm, rs1, 0x0, *rd, 0b1100111)
        }

        (Op::U { opcode }, 
            [Reg(rd), Comma, Int(imm)]
        ) => {
            encode::u_type(*imm, *rd, opcode)
        }

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

    #[test]
    fn store_shape() {
        let instr = [id("sw"), Reg(2), Comma, Int(-4), LParen, Reg(1), RParen];
        assert_eq!(encode_line(&instr, 0, &vec![]), Ok(0xFE20AE23));
    }

    #[test]
    fn implicit_offsets() {
        let instr_exp = [id("lw"), Reg(5), Comma, Int(0), LParen, Reg(2), RParen];
        let instr_imp = [id("lw"), Reg(5), Comma, LParen, Reg(2), RParen];
        assert_eq!(encode_line(&instr_exp, 0, &vec![]), encode_line(&instr_imp, 0, &vec![]));
    }

    #[test]
    fn jumps() {
        let symbols = vec![("f".to_string(), 8)];
        let jal = [id("jal"), Reg(1), Comma, id("f")];
        assert_eq!(encode_line(&jal, 0, &symbols), Ok(0x008000EF));
        let ret = [id("jalr"), Reg(0), Comma, Int(0), LParen, Reg(1), RParen];
        assert_eq!(encode_line(&ret, 0, &vec![]), Ok(0x00008067));
    }

    #[test]
    fn upper() {
        let lui = [id("lui"), Reg(5), Comma, Int(0x12345)];
        assert_eq!(encode_line(&lui, 0, &vec![]), Ok(0x123452B7));
        let auipc = [id("auipc"), Reg(5), Comma, Int(0x12345)];
        assert_eq!(encode_line(&auipc, 0, &vec![]), Ok(0x12345297));
    }

    #[test]
    fn sltiu_and_ebreak() {
        let sltiu = [id("sltiu"), Reg(1), Comma, Reg(2), Comma, Int(1)];
        assert_eq!(encode_line(&sltiu, 0, &vec![]), Ok(0x00113093));
        assert_eq!(encode_line(&[id("ebreak")], 0, &vec![]), Ok(0x00100073));
    }

    #[test]
    fn pass2_line_number() {
        let lines = vec![vec![], vec![id("add"), Reg(1)]];
        let e = pass2(&lines, &vec![]).unwrap_err();
        assert!(e.starts_with("line 2: assembly error: "), "{e}");
    }

    #[test]
    fn mem_operand_forms() {
        let sw = [id("sw"), Reg(2), Comma, LParen, Reg(1), RParen];
        assert_eq!(encode_line(&sw, 0, &vec![]), Ok(0x0020A023));
        let ret_short = [id("jalr"), Reg(0), Comma, LParen, Reg(1), RParen];
        assert_eq!(encode_line(&ret_short, 0, &vec![]), Ok(0x00008067));
        let ret_three = [id("jalr"), Reg(0), Comma, Reg(1), Comma, Int(0)];
        assert_eq!(encode_line(&ret_three, 0, &vec![]), Ok(0x00008067));
        let bad = [id("lw"), Reg(5), Comma, Int(4), Reg(2)];
        let e = encode_line(&bad, 0, &vec![]).unwrap_err();
        assert!(e.contains("expected offset(register)"), "{e}");
    }
}
