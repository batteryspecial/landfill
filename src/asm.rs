mod classify;
mod encode;
pub mod types;
pub mod scan;
pub mod token;

use std::{collections::HashMap};
use token::Token;
use std::hash::{BuildHasherDefault, Hasher};

const ERR: &str = "assembly error";

// Symbol table: symbols is the output, in definition order
pub type Symbols<'a> = Vec<(&'a str, u32)>;
// Index records where label X actually is
type Index<'a> = HashMap<&'a str, u32, BuildHasherDefault<WordHash>>;

#[derive(Default)]
struct WordHash(u64);

// Implement a ligher hash function for short label keys
impl Hasher for WordHash {
    // FNV-1a (FxHash)
    fn write(&mut self, bytes: &[u8]) {
        for w in bytes.chunks(8) {
            let mut buffer = [0; 8];
            buffer[..w.len()].copy_from_slice(w);
            self.0 = (self.0.rotate_left(5) ^ u64::from_le_bytes(buffer)).wrapping_mul(
                0x517c_c1b7_2722_0a95
            )
        }
    }
    fn write_u8(&mut self, bytes: u8) {
        self.0 = (self.0.rotate_left(5) ^ bytes as u64).wrapping_mul(
            0x517c_c1b7_2722_0a95
        )
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

// A branch or jal whose label is not defined yet; encoded once the label is known
struct Fixup<'a> {
    line: usize,
    pc: u32,
    label: &'a str,
    op: Op,
    rd_rs1: u32,
    rs2: u32,
}

// Everything a line can add to; shared by the fast and general paths
struct State<'a> {
    index: Index<'a>,
    order: Vec<(&'a str, u32, usize)>, // label, pc, line
    fixups: Vec<Fixup<'a>>,
    out: Vec<u8>,
    pc: u32,
}

// One chunk of whole lines, assembled on its own
struct Part<'a> {
    st: State<'a>,
    lines: usize,
    scan_err: Option<(usize, String)>,
    pass1_err: Option<(usize, String)>,
    pass2_err: Option<(usize, String)>,
}

// Inputs below this stay on one thread; above it, chunks of about this size
const CHUNK: usize = 256 << 10;

pub fn assemble(src: &str) -> Result<(Vec<u8>, Symbols<'_>), String> {
    assemble_bytes(src.as_bytes()).expect("a str is UTF-8")
}

// Assemble raw source and none if it is not UTF-8
pub fn assemble_bytes(src: &[u8]) -> Option<Result<(Vec<u8>, Symbols<'_>), String>> {
    // Lines are independent apart from labels, pcs and error order

    let part = |chunk| std::str::from_utf8(chunk).ok().map(assemble_part::<true>);
    let parts: Vec<Option<Part>> = if src.len() <= CHUNK {
        vec![part(src)]
    } else {
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < src.len() {
            // cut after a '\n'
            let end = scan::skip_line(src, (start + CHUNK).min(src.len()));
            chunks.push(&src[start..end]);
            start = end;
        }
        let threads = std::thread::available_parallelism().map_or(
            1, |n| n.get()
        ).min(chunks.len());
        let next = std::sync::atomic::AtomicUsize::new(0);

        let mut parts: Vec<(usize, Option<Part>)> = std::thread::scope(|s| {
            let workers: Vec<_> = (0..threads).map(|_| s.spawn(|| {
                let mut done = Vec::new();
                loop {
                    // dynamically take chunks as they come, so slower cores take fewer
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(chunk) = chunks.get(k) else { return done };
                    done.push((k, part(chunk)));
                }
            })).collect();
            workers.into_iter().flat_map(
                |w| w.join().unwrap()
            ).collect()
        });

        parts.sort_by_key(|&(k, _)| k);
        parts.into_iter().map(|(_, p)| p).collect()
    };
    Some(merge(parts.into_iter().collect::<Option<_>>()?))
}

fn merge<'a>(mut parts: Vec<Part<'a>>) -> Result<(Vec<u8>, Symbols<'a>), String> {
    let at = |off: usize, (line, e): &(usize, String)| format!("line {}: {e}", off + line);

    // first line of each part, and pc of each part's first instruction
    let mut line_off = Vec::with_capacity(parts.len());
    let mut pc_off = Vec::with_capacity(parts.len());
    let (mut lines, mut pc) = (0, 0u32);
    for p in &parts {
        line_off.push(lines);
        pc_off.push(pc);
        lines += p.lines;
        pc += p.st.pc;
    }

    for (p, &off) in parts.iter().zip(&line_off) {
        if let Some(e) = &p.scan_err {
            return Err(at(off, e));
        }
    }

    // pass 1: c
    let n_labels = parts.iter().map(|p| p.st.order.len()).sum();
    let mut index = Index::with_capacity_and_hasher(n_labels, Default::default());
    let mut order = Vec::with_capacity(n_labels);
    for (k, p) in parts.iter().enumerate() {
        for &(name, addr, line) in &p.st.order {
            if index.insert(name, pc_off[k] + addr).is_some() {
                return Err(format!("line {}: {ERR}: duplicate label '{name}'", line_off[k] + line));
            }
            order.push((name, pc_off[k] + addr));
        }
        if let Some(e) = &p.pass1_err {
            return Err(at(line_off[k], e));
        }
    }

    // pass 2: labels a part could not see; all of a part's fixups precede its error
    for (k, p) in parts.iter_mut().enumerate() {
        for f in &p.st.fixups {
            let word = match index.get(f.label) {
                Some(&addr) => encode_jump(f.op, addr as i64 - (pc_off[k] + f.pc) as i64, f.rd_rs1, f.rs2),
                None => Err(format!("{ERR}: undefined label '{}'", f.label)),
            }.map_err(|e| at(line_off[k], &(f.line, e)))?;
            let at = f.pc as usize;
            p.st.out[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        if let Some(e) = &p.pass2_err {
            return Err(at(line_off[k], e));
        }
    }

    let mut out = Vec::with_capacity(pc as usize);
    for p in &parts {
        out.extend_from_slice(&p.st.out);
    }
    Ok((out, order))
}

// FAST is false only in tests, to check the fast path against the general one
fn assemble_part<const FAST: bool>(src: &str) -> Part<'_> {
    let mut st = State {
        index: Index::default(),
        order: Vec::new(),
        fixups: Vec::new(),
        out: Vec::with_capacity(src.len() / 8),
        pc: 0,
    };
    let mut tokens = Vec::with_capacity(16);
    let mut ends: classify::Ends = [0; classify::LINE_BATCH + 64];
    let (mut pass1_err, mut pass2_err) = (None, None);
    let (mut pos, mut line) = (0, 0);

    while pos < src.len() {
        let n = classify::line_ends(src.as_bytes(), pos, &mut ends);

        for &next in &ends[..n] {
            line += 1;
            let start = pos;
            pos = next;

            // Well-formed lines take the fast path; anything else is redone below
            if FAST && pass1_err.is_none() && pass2_err.is_none() && encode::line(src, start, line, &mut st) {
                continue;
            }

            tokens.clear();
            if let Err(e) = scan::scan_line(src, start, &mut tokens) {
                // nothing after a scanning error matters
                return Part { st, lines: line, scan_err: Some((line, e)), pass1_err, pass2_err };
            }
            if pass1_err.is_some() {
                continue; // only a scanning error can still win
            }

            // 1. Split leading labels from the instruction
            let n_labels = tokens.iter().take_while(
                |t| matches!(t, Token::Label(_))
            ).count();
            let instr = &tokens[n_labels..];

            // 2. Ensure no labels appear after the instruction
            if instr.iter().any(|t| matches!(t, Token::Label(_))) {
                pass1_err = Some((line, format!("{ERR}: label after instruction")));
                continue;
            }

            // 3. Record every label at the current pc
            for t in &tokens[..n_labels] {
                if let Token::Label(name) = *t {
                    if st.index.insert(name, st.pc).is_some() {
                        pass1_err = Some((line, format!("{ERR}: duplicate label '{name}'")));
                        break;
                    }
                    st.order.push((name, st.pc, line));
                }
            }
            if pass1_err.is_some() || instr.is_empty() {
                continue;
            }

            // 4. Encode, deferring labels not seen yet
            if pass2_err.is_none() {
                let pc = st.pc;
                match encode_line(instr, pc, &st.index) {
                    Ok(Encoded::Word(word)) => st.out.extend_from_slice(&word.to_le_bytes()),
                    Ok(Encoded::Forward { label, op, rd_rs1, rs2 }) => {
                        st.fixups.push(Fixup { line, pc, label, op, rd_rs1, rs2 });
                        st.out.extend_from_slice(&[0; 4]);
                    }
                    Err(e) => pass2_err = Some((line, e)),
                }
            }
            st.pc += 4;
        }
    }

    // Labels defined in this chunk resolve here, in parallel
    if pass1_err.is_none() {
        let mut kept = 0;
        for k in 0..st.fixups.len() {
            let f = &st.fixups[k];
            let Some(&addr) = st.index.get(f.label) else {
                st.fixups.swap(kept, k);
                kept += 1;
                continue;
            };
            match encode_jump(f.op, addr as i64 - f.pc as i64, f.rd_rs1, f.rs2) {
                Ok(word) => st.out[f.pc as usize..f.pc as usize + 4].copy_from_slice(&word.to_le_bytes()),
                Err(e) => {
                    pass2_err = Some((f.line, e));
                    break;
                }
            }
        }
        st.fixups.truncate(kept);
    }

    Part { st, lines: line, scan_err: None, pass1_err, pass2_err }
}

#[derive(Debug, Clone, Copy, PartialEq)]
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

const OP_NAMES: [(&str, Op); 39] = {
    use Op::*;
    [
        ("add", R { funct3: 0x0, funct7: 0x00 }),
        ("sub", R { funct3: 0x0, funct7: 0x20 }),
        ("xor", R { funct3: 0x4, funct7: 0x00 }),
        ("or", R { funct3: 0x6, funct7: 0x00 }),
        ("and", R { funct3: 0x7, funct7: 0x00 }),
        ("sll", R { funct3: 0x1, funct7: 0x00 }),
        ("srl", R { funct3: 0x5, funct7: 0x00 }),
        ("sra", R { funct3: 0x5, funct7: 0x20 }),
        ("slt", R { funct3: 0x2, funct7: 0x00 }),
        ("sltu", R { funct3: 0x3, funct7: 0x00 }),
        ("addi", I { funct3: 0x0 }),
        ("xori", I { funct3: 0x4 }),
        ("ori", I { funct3: 0x6 }),
        ("andi", I { funct3: 0x7 }),
        ("slti", I { funct3: 0x2 }),
        ("sltiu", I { funct3: 0x3 }),
        ("slli", Shift { funct3: 0x1, funct7: 0x00 }),
        ("srai", Shift { funct3: 0x5, funct7: 0x20 }),
        ("srli", Shift { funct3: 0x5, funct7: 0x00 }),
        ("lw", Load { funct3: 0x2 }),
        ("lb", Load { funct3: 0x0 }),
        ("lh", Load { funct3: 0x1 }),
        ("lbu", Load { funct3: 0x4 }),
        ("lhu", Load { funct3: 0x5 }),
        ("sb", Store { funct3: 0x0 }),
        ("sh", Store { funct3: 0x1 }),
        ("sw", Store { funct3: 0x2 }),
        ("beq", Branch { funct3: 0x0 }),
        ("bne", Branch { funct3: 0x1 }),
        ("blt", Branch { funct3: 0x4 }),
        ("bge", Branch { funct3: 0x5 }),
        ("bltu", Branch { funct3: 0x6 }),
        ("bgeu", Branch { funct3: 0x7 }),
        ("jal", Jal),
        ("jalr", Jalr),
        ("lui", U { opcode: 0x37 }),
        ("auipc", U { opcode: 0x17 }),
        ("ecall", Fixed(0x00000073)),
        ("ebreak", Fixed(0x00100073)),
    ]
};

const OP_M: u64 = 0x9f428d80a545a27d;

static OPS: [(u64, Op); 64] = {
    let mut t = [(0, Op::Fixed(0)); 64];
    let mut i = 0;
    while i < OP_NAMES.len() {
        let k = scan::key(OP_NAMES[i].0.as_bytes());
        let h = scan::slot(k, OP_M, 6);
        assert!(t[h].0 == 0, "mnemonic hash collision");
        t[h] = (k, OP_NAMES[i].1);
        i += 1;
    }
    t
};

fn op_of(k: u64) -> Option<Op> {
    let (tk, op) = OPS[scan::slot(k, OP_M, 6)];
    (tk == k).then_some(op)
}

fn lookup(mnemonic: &str) -> Option<Op> {
    op_of(scan::word_key(mnemonic)?)
}

enum Target<'a> {
    Offset(i64),
    Forward(&'a str),
}

fn target<'a>(target: &Token<'a>, pc: u32, index: &Index) -> Result<Target<'a>, String> {
    match *target {
        Token::Int(n) => Ok(Target::Offset(n)),
        Token::Id(name) => Ok(match index.get(name) {
            Some(&addr) => Target::Offset(addr as i64 - pc as i64),
            None => Target::Forward(name),
        }),
        other => Err(format!("{ERR}: expected label or offset, received '{other}'")),
    }
}

fn encode_jump(op: Op, offset: i64, rd_rs1: u32, rs2: u32) -> Result<u32, String> {
    match op {
        Op::Branch { funct3 } => types::b_type(offset, rs2, rd_rs1, funct3),
        _ => types::j_type(offset, rd_rs1),
    }
}

#[derive(Debug, PartialEq)]
enum Encoded<'a> {
    Word(u32),
    Forward { label: &'a str, op: Op, rd_rs1: u32, rs2: u32 },
}

fn jump<'a>(op: Op, t: Target<'a>, rd_rs1: u32, rs2: u32) -> Result<Encoded<'a>, String> {
    match t {
        Target::Offset(offset) => encode_jump(op, offset, rd_rs1, rs2).map(Encoded::Word),
        Target::Forward(label) => Ok(Encoded::Forward { label, op, rd_rs1, rs2 }),
    }
}

fn mem_operand(ops: &[Token]) -> Result<(i64, u32), String> {
    match ops {
        [Token::Int(imm), Token::LParen, Token::Reg(rs1), Token::RParen] => Ok((*imm, *rs1)),
        [Token::LParen, Token::Reg(rs1), Token::RParen] => Ok((0, *rs1)),
        _ => Err(format!("{ERR}: expected format offset(register)")),
    }
}

fn encode_line<'a>(instr: &[Token<'a>], pc: u32, index: &Index) -> Result<Encoded<'a>, String> {
    use Token::{Comma, Int, Reg};

    let Token::Id(mnemonic) = instr[0] else {
        return Err(format!("{ERR}: expected instruction, received '{}'", instr[0]));
    };

    match (mnemonic, &instr[1..]) {
        // mv rd, rs -> addi rd, rs, 0
        ("mv", [Reg(rd), Comma, Reg(rs)]) => {
            return types::i_type(0, *rs, 0x0, *rd, 0b0010011).map(Encoded::Word)
        }
        ("j", [target]) => {
            return jump(Op::Jal, self::target(target, pc, index)?, 0, 0)
        }
        _ => {}
    }

    let op = lookup(mnemonic).ok_or_else(|| format!("{ERR}: unknown instruction '{mnemonic}'"))?;

    let word = match (op, &instr[1..]) {
        (Op::R { funct3, funct7 }, 
            [Reg(rd), Comma, Reg(rs1), Comma, Reg(rs2)]
        ) => {
            Ok(types::r_type(funct7, *rs2, *rs1, funct3, *rd, 0b0110011))
        }

        (Op::I { funct3 }, 
            [Reg(rd), Comma, Reg(rs1), Comma, Int(imm)]
        ) => {
            types::i_type(*imm, *rs1, funct3, *rd, 0b0010011)
        }

        (Op::Shift { funct3, funct7 }, 
            [Reg(rd), Comma, Reg(rs1), Comma, Int(shamt)]
        ) => {
            if !(0..=31).contains(shamt) {
                return Err(format!("{ERR}: shift amount '{shamt}' out of range"));
            }
            types::i_type((funct7 << 5) as i64 | *shamt, *rs1, funct3, *rd, 0b0010011)
        }

        (Op::Load { funct3 }, 
            [Reg(rd), Comma, mem @ ..]
        ) => {
            let (imm, rs1) = mem_operand(mem)?;
            types::i_type(imm, rs1, funct3, *rd, 0b0000011)
        }
        
        (Op::Store { funct3 }, 
            [Reg(rs2), Comma, mem @ ..]
        ) => {
            let (imm, rs1) = mem_operand(mem)?;
            types::s_type(imm, *rs2, rs1, funct3)
        }

        (Op::Branch { .. },
            [Reg(rs1), Comma, Reg(rs2), Comma, target]
        ) => {
            return jump(op, self::target(target, pc, index)?, *rs1, *rs2)
        }

        (Op::Jal, 
            [Reg(rd), Comma, target]
        ) => {
            return jump(op, self::target(target, pc, index)?, *rd, 0)
        }

        (Op::Jalr,
            [Reg(rd), Comma, Reg(rs1), Comma, Int(imm)]
        ) => {
            types::i_type(*imm, *rs1, 0x0, *rd, 0b1100111)
        }

        (Op::Jalr,
            [Reg(rd), Comma, mem @ ..]
        ) => {
            let (imm, rs1) = mem_operand(mem)?;
            types::i_type(imm, rs1, 0x0, *rd, 0b1100111)
        }

        (Op::U { opcode }, 
            [Reg(rd), Comma, Int(imm)]
        ) => {
            types::u_type(*imm, *rd, opcode)
        }

        (Op::Fixed(word), []) => Ok(word),
        _ => Err(format!("{ERR}: invalid operands for '{mnemonic}'")),
    };
    word.map(Encoded::Word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Token<'_> {
        Token::Id(s)
    }
    fn encode_line(instr: &[Token], pc: u32, symbols: &[(&str, u32)]) -> Result<u32, String> {
        let index: Index = symbols.iter().copied().collect();
        match super::encode_line(instr, pc, &index)? {
            Encoded::Word(w) => Ok(w),
            Encoded::Forward { label, .. } => Err(format!("{ERR}: undefined label '{label}'")),
        }
    }
    use Token::{Comma, Int, LParen, RParen, Reg};

    #[test]
    fn pass1_addresses() {
        let (_, symbols) = assemble("a:\n\necall\nb: c: ecall\nd:\n").unwrap();
        let want = vec![("a", 0), ("b", 4), ("c", 4), ("d", 8)];
        assert_eq!(symbols, want);
    }

    #[test]
    fn pass1_duplicate() {
        assert!(assemble("a:\na: ecall\n").is_err());
        assert!(assemble("a: a: ecall\n").is_err());
    }

    #[test]
    fn pass1_label_after_instr() {
        let e = assemble("\necall a:\n").unwrap_err();
        assert!(e.starts_with("line 2: assembly error: "), "{e}");
    }

    #[test]
    fn forward_labels() {
        let (bytes, _) = assemble("beq x1, x2, f\njal ra, f\nf: ecall\n").unwrap();
        let want: Vec<u8> = [0x00208463u32, 0x004000EF, 0x00000073]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        assert_eq!(bytes, want);
    }

    #[test]
    fn error_precedence() {
        // scanning beats labels beats encoding; earliest encoding error wins
        let e = assemble("add x1\na:\na:\n$\n").unwrap_err();
        assert!(e.starts_with("line 4: scanning error"), "{e}");
        let e = assemble("add x1\na:\na:\n").unwrap_err();
        assert!(e.starts_with("line 3: assembly error: duplicate"), "{e}");
        let e = assemble("jal x1, nowhere\nadd x1\n").unwrap_err();
        assert!(e.starts_with("line 1: assembly error: undefined"), "{e}");
        let e = assemble("add x1\njal x1, nowhere\n").unwrap_err();
        assert!(e.starts_with("line 1: assembly error: invalid operands"), "{e}");
    }

    #[test]
    fn r_shape() {
        let instr = [id("add"), Reg(3), Comma, Reg(1), Comma, Reg(2)];
        assert_eq!(encode_line(&instr, 0, &[]), Ok(0x002081B3));
    }

    #[test]
    fn load_shape() {
        let instr = [id("lw"), Reg(5), Comma, Int(-4), LParen, Reg(2), RParen];
        assert_eq!(encode_line(&instr, 0, &[]), Ok(0xFFC12283));
    }

    #[test]
    fn shift_imm() {
        let instr = [id("srai"), Reg(1), Comma, Reg(2), Comma, Int(3)];
        assert_eq!(encode_line(&instr, 0, &[]), Ok(0x40315093));
        let instr = [id("slli"), Reg(1), Comma, Reg(2), Comma, Int(32)];
        assert!(encode_line(&instr, 0, &[]).is_err());
    }

    #[test]
    fn branch_to_label() {
        let symbols = [("loop", 0)];
        let instr = [id("beq"), Reg(1), Comma, Reg(2), Comma, id("loop")];
        assert_eq!(encode_line(&instr, 8, &symbols), Ok(0xFE208CE3));
    }

    #[test]
    fn no_operands() {
        assert_eq!(encode_line(&[id("ecall")], 0, &[]), Ok(0x00000073));
    }

    #[test]
    fn errors() {
        let undefined = [id("jal"), Reg(1), Comma, id("nowhere")];
        assert!(encode_line(&undefined, 0, &[]).is_err());
        let wrong_shape = [id("add"), Reg(1), Comma, Reg(2)];
        assert!(encode_line(&wrong_shape, 0, &[]).is_err());
        let unknown = [id("mul"), Reg(1), Comma, Reg(2), Comma, Reg(3)];
        assert!(encode_line(&unknown, 0, &[]).is_err());
    }

    #[test]
    fn store_shape() {
        let instr = [id("sw"), Reg(2), Comma, Int(-4), LParen, Reg(1), RParen];
        assert_eq!(encode_line(&instr, 0, &[]), Ok(0xFE20AE23));
    }

    #[test]
    fn implicit_offsets() {
        let instr_exp = [id("lw"), Reg(5), Comma, Int(0), LParen, Reg(2), RParen];
        let instr_imp = [id("lw"), Reg(5), Comma, LParen, Reg(2), RParen];
        assert_eq!(encode_line(&instr_exp, 0, &[]), encode_line(&instr_imp, 0, &[]));
    }

    #[test]
    fn jumps() {
        let symbols = [("f", 8)];
        let jal = [id("jal"), Reg(1), Comma, id("f")];
        assert_eq!(encode_line(&jal, 0, &symbols), Ok(0x008000EF));
        let ret = [id("jalr"), Reg(0), Comma, Int(0), LParen, Reg(1), RParen];
        assert_eq!(encode_line(&ret, 0, &[]), Ok(0x00008067));
    }

    #[test]
    fn upper() {
        let lui = [id("lui"), Reg(5), Comma, Int(0x12345)];
        assert_eq!(encode_line(&lui, 0, &[]), Ok(0x123452B7));
        let auipc = [id("auipc"), Reg(5), Comma, Int(0x12345)];
        assert_eq!(encode_line(&auipc, 0, &[]), Ok(0x12345297));
    }

    #[test]
    fn sltiu_and_ebreak() {
        let sltiu = [id("sltiu"), Reg(1), Comma, Reg(2), Comma, Int(1)];
        assert_eq!(encode_line(&sltiu, 0, &[]), Ok(0x00113093));
        assert_eq!(encode_line(&[id("ebreak")], 0, &[]), Ok(0x00100073));
    }

    #[test]
    fn pass2_line_number() {
        let e = assemble("\nadd x1\n").unwrap_err();
        assert!(e.starts_with("line 2: assembly error: "), "{e}");
    }

    #[test]
    fn mem_operand_forms() {
        let sw = [id("sw"), Reg(2), Comma, LParen, Reg(1), RParen];
        assert_eq!(encode_line(&sw, 0, &[]), Ok(0x0020A023));
        let ret_short = [id("jalr"), Reg(0), Comma, LParen, Reg(1), RParen];
        assert_eq!(encode_line(&ret_short, 0, &[]), Ok(0x00008067));
        let ret_three = [id("jalr"), Reg(0), Comma, Reg(1), Comma, Int(0)];
        assert_eq!(encode_line(&ret_three, 0, &[]), Ok(0x00008067));
        let bad = [id("lw"), Reg(5), Comma, Int(4), Reg(2)];
        let e = encode_line(&bad, 0, &[]).unwrap_err();
        assert!(e.contains("expected format offset(register)"), "{e}");
    }

    #[test]
    fn whole_program() {
        let (bytes, symbols) = assemble("loop: addi t0, t0, -1\nbne t0, zero, loop\n").unwrap();
        let want: Vec<u8> = [0xFFF28293u32, 0xFE029EE3]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        assert_eq!(bytes, want);
        assert_eq!(symbols, vec![("loop", 0)]);
    }

    // The fast path must agree with the general path on every line: same words,
    // symbols and error text. Lines are built from pieces that hit its edge cases.
    #[test]
    fn fast_path_matches_general() {
        fn general(src: &str) -> Result<(Vec<u8>, Symbols<'_>), String> {
            merge(vec![assemble_part::<false>(src)])
        }
        let pieces = [
            "add", "addi", "sub", "slli", "srai", "lw", "sw", "sb", "beq", "bne", "jal", "jalr", "lui",
            "auipc", "ecall", "ebreak", "mul", "ADD", "add_", "x0", "x1", "x31", "x32", "x01", "zero",
            "ra", "sp", "t0", "s11", "a7", "fp", "0", "-1", "+5", "42", "-2048", "2047", "2048", "4095",
            "-4096", "0x7FF", "0x800", "-0x10", "0X1f", "0x", "010", "00", "99999999", "123456789",
            "-99999999999999999999", "31", "32", "loop", "done", "_x", "x1-5", "a-b", "5x", "loop:",
            "t0:", "x32:", "done:", ",", ",", ",", "(", ")", "(sp)", "4(sp)", "-8(x2)", "#", "# c",
            ":", "$", "\t", " ", "  ", "                                ", "é", "\u{a0}", "\r",
            "very_long_label_name_that_overflows",
        ];
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut rand = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for _ in 0..20_000 {
            let mut src = String::new();
            for _ in 0..1 + rand(4) {
                // mostly well-formed shapes, so the fast path is exercised, plus noise
                let words = 1 + rand(9);
                for k in 0..words {
                    src += pieces[rand(pieces.len())];
                    if rand(3) != 0 && k + 1 < words {
                        src += [" ", ", ", ",", "  ", "\t"][rand(5)];
                    }
                }
                if rand(8) != 0 {
                    src += ["\n", "  # note\n", "\r\n", "\n\n"][rand(4)];
                }
            }
            assert_eq!(assemble(&src), general(&src), "{src:?}");
        }
        // real programs, word for word
        for src in [
            "loop: addi t0, t0, -1\nbne t0, zero, loop\n",
            "        addi  sp, sp, -16                # ff010113\n        sw    ra, 12(sp)\n",
            "a: b: c:\n  jal ra, f\n  beq x1, x2, a\nf: jalr zero, 0(ra)\nlui a0, 0xFFFFF\n",
            "loop10:   beq   a1, zero, done10             # 0\ndone10: ecall\n",
            "        lw    t1, 0(a0)                  # 00052303\nlw x1,(x2)\njalr x1, x2, 4\nsw x1, -2048(x2)",
        ] {
            assert_eq!(assemble(src), general(src), "{src:?}");
        }
    }

    #[test]
    fn scan_error_stops_assembly() {
        let e = assemble("add x1, x2, x3\nadd x1, x2, $3\n").unwrap_err();
        assert!(e.starts_with("line 2: scanning error: "), "{e}");
    }
}
