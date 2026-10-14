use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples");

fn expected_words(src: &str) -> Vec<u32> {
    src.lines()
        .filter_map(|line| line.split_once('#'))
        .filter_map(|(_, comment)| comment.split_whitespace().next())
        .filter(|w| w.len() == 8 && w.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|w| u32::from_str_radix(w, 16).unwrap())
        .collect()
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

fn sample_files(dir: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "S"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no samples in {dir}");
    files
}

#[test]
fn samples_match_reference_encodings() {
    for path in sample_files(SAMPLES) {
        let src = fs::read_to_string(&path).unwrap();
        let want = expected_words(&src);
        assert!(!want.is_empty(), "{} has no expected words", path.display());

        let (bytes, _) = landfill::asm::assemble(&src)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let got = words(&bytes);

        assert_eq!(got.len(), want.len(), "{}: word count", path.display());
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(g, w, "{}: word {i}: got {g:#010x}, want {w:#010x}", path.display());
        }
    }
}

#[test]
fn error_samples_fail_with_expected_message() {
    for path in sample_files(&format!("{SAMPLES}/errors")) {
        let src = fs::read_to_string(&path).unwrap();
        let want = src
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("# error: "))
            .unwrap_or_else(|| panic!("{} has no '# error:' header", path.display()));

        let e = landfill::asm::assemble(&src).unwrap_err();
        assert!(e.contains(want), "{}: got '{e}', want '{want}'", path.display());
    }
}

fn landfill(args: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_landfill"))
        .args(args)
        .output()
        .unwrap()
}

fn scratch_copy(sample: &str, test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("landfill-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dest = dir.join(Path::new(sample).file_name().unwrap());
    fs::copy(format!("{SAMPLES}/{sample}"), &dest).unwrap();
    dest
}

#[test]
fn cli_writes_bin_next_to_input() {
    let asm = scratch_copy("program.S", "bin");
    let out = landfill(&[&asm]);

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stdout.is_empty());

    let bin = fs::read(asm.with_extension("bin")).unwrap();
    let want = expected_words(&fs::read_to_string(&asm).unwrap());
    assert_eq!(words(&bin), want);
}

#[test]
fn cli_prints_symbol_table() {
    let asm = scratch_copy("program.S", "symbols");
    for flag in ["-s", "-st", "--symboltable"] {
        let out = landfill(&[Path::new(flag), &asm]);
        assert!(out.status.success(), "{flag}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "loop 0x00000014\ndone 0x0000002c\n",
            "{flag}"
        );
    }
}

#[test]
fn cli_error_writes_no_bin() {
    let asm = scratch_copy("errors/scan.S", "error");
    let out = landfill(&[&asm]);

    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("line 3: scanning error"));
    assert!(!asm.with_extension("bin").exists());
}

#[test]
fn cli_version() {
    let out = landfill(&[Path::new("--version")]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn cli_bad_arguments_exit_2() {
    let asm = scratch_copy("program.S", "usage");
    assert_eq!(landfill(&[]).status.code(), Some(2));
    assert_eq!(landfill(&[Path::new("-x"), &asm]).status.code(), Some(2));
    assert_eq!(landfill(&[&asm, &asm]).status.code(), Some(2));
    assert_eq!(landfill(&[Path::new("--version"), &asm]).status.code(), Some(2));
}
