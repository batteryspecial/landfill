const ERR: &str = "encoding error";

pub fn r_type(funct7: u32, rs2: u32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    (funct7 << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
}

fn range_imm(imm: i64, n: u32) -> bool {
    matches!(imm >> (n - 1), 0 | -1)
}

pub fn i_type(imm: i64, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> Result<u32, String> {
    // range-check imm (-2048..=2047), then pack
    if !range_imm(imm, 12) {
        return Err(format!("{ERR}: out-of-range immediate {imm}"))
    }

    Ok(((imm as u32 & 0xFFF) << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode)
}

pub fn s_type(imm: i64, rs2: u32, rs1: u32, funct3: u32) -> Result<u32, String> {
    // range-check, split imm[11:5] / imm[4:0]
    // opcode is always 0b0100011.
    if !range_imm(imm, 12) {
        return Err(format!("{ERR}: out-of-range immediate {imm}"));
    }

    let imm_u = imm as u32 & 0xFFF;
    let imm_11_5 = imm_u >> 5 & 0x7F;
    let imm_4_0 = imm_u & 0x1F;

    let opcode = 0b0100011;

    Ok((imm_11_5 << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (imm_4_0 << 7) | opcode)
}

pub fn b_type(offset: i64, rs2: u32, rs1: u32, funct3: u32) -> Result<u32, String> {
    // range-check (-4096..=4094), reject odd offsets, scatter bits
    // opcode is always 0b1100011

    if !range_imm(offset, 13) { 
        return Err(format!("{ERR}: out-of-range branch target {offset}"));
    }
    if offset % 2 != 0 {
        return Err(format!("{ERR}: invalid odd branch target {offset}"));
    }

    let offset_u = offset as u32 & 0x1FFF; // 13-bit in 12-bit imm since bit 0 is implied
    let offset_12 = offset_u >> 12 & 0x1;
    let offset_11 = offset_u >> 11 & 0x1;
    let offset_10_5 = offset_u >> 5 & 0x3F;
    let offset_4_1 = offset_u >> 1 & 0xF;

    let opcode = 0b1100011;

    Ok((offset_12 << 31) | (offset_10_5 << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (offset_4_1 << 8) | (offset_11 << 7) | opcode)
}

pub fn u_type(imm: i64, rd: u32, opcode: u32) -> Result<u32, String> {
    // range-check (0..=0xFFFFF), place in bits 31:12
    if  !(0..=0xFFFFF).contains(&imm) {
        return Err(format!("{ERR}: out-of-range immediate {imm}"));
    }

    Ok(((imm as u32 & 0xFFFFF) << 12) | (rd << 7) | opcode)
}

pub fn j_type(offset: i64, rd: u32) -> Result<u32, String> {
    // range-check (-1 MiB..=1 MiB - 2), reject odd offsets, scatter bits
    // opcode is always 0b1101111
    if !range_imm(offset, 21) {
        return Err(format!("{ERR}: out-of-range jump target {offset}"));
    }
    if offset % 2 != 0 {
        return Err(format!("{ERR}: invalid odd jump target {offset}"));
    }

    let offset_u = offset as u32 & 0x1FFFFF; // 21-bit offset, bit 0 implied
    let offset_20 = offset_u >> 20 & 0x1;
    let offset_19_12 = offset_u >> 12 & 0xFF;
    let offset_11 = offset_u >> 11 & 0x1;
    let offset_10_1 = offset_u >> 1 & 0x3FF;

    let opcode = 0b1101111;

    Ok((offset_20 << 31) | (offset_10_1 << 21) | (offset_11 << 20) | (offset_19_12 << 12) | (rd << 7) | opcode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r_add() {
        assert_eq!(r_type(0, 2, 1, 0, 3, 0b0110011), 0x002081B3);
    }

    #[test]
    fn i_addi_negative() {
        assert_eq!(i_type(-1, 0, 0, 1, 0b0010011), Ok(0xFFF00093));
        assert_eq!(i_type(1, 0, 0, 1, 0b0010011), Ok(0x00100093));
    }

    #[test]
    fn s_sw_negative() {
        assert_eq!(s_type(-4, 2, 1, 0b010), Ok(0xFE20AE23));
    }

    #[test]
    fn b_beq_backwards() {
        assert_eq!(b_type(-8, 2, 1, 0), Ok(0xFE208CE3));
    }

    #[test]
    fn u_lui() {
        assert_eq!(u_type(0x12345, 5, 0b0110111), Ok(0x123452B7));
    }

    #[test]
    fn j_jal_backwards() {
        assert_eq!(j_type(-4, 0), Ok(0xFFDFF06F));
    }

    #[test]
    fn range_errors() {
        assert!(i_type(2048, 0, 0, 1, 0b0010011).is_err());
        assert!(i_type(-2049, 0, 0, 1, 0b0010011).is_err());
        assert!(s_type(2048, 2, 1, 0b010).is_err());
        assert!(b_type(4096, 2, 1, 0).is_err());
        assert!(b_type(-4098, 2, 1, 0).is_err());
        assert!(u_type(0x100000, 5, 0b0110111).is_err());
        assert!(u_type(-1, 5, 0b0110111).is_err());
        assert!(j_type(1 << 20, 0).is_err());
    }

    #[test]
    fn odd_offsets() {
        assert!(b_type(3, 2, 1, 0).is_err());
        assert!(b_type(-3, 2, 1, 0).is_err());
        assert!(j_type(-1, 0).is_err());
    }

    #[test]
    fn error_prefix() {
        let e = j_type(-1, 0).unwrap_err();
        assert!(e.starts_with("encoding error: "), "{e}");
    }
}
