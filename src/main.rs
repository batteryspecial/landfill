use std::path::Path;
use std::process::exit;

const USAGE: &str = "USAGE: landfill [-s | -st | --symboltable] FILE\n       landfill --version";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let (show_symbols, path) = match args.as_slice() {
        [flag] if flag == "--version" => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return;
        }
        [path] if !path.starts_with('-') => (false, path),
        [flag, path] if flag == "-s" || flag == "-st" || flag == "--symboltable" => (true, path),
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    };

    if let Err(e) = run(path, show_symbols) {
        eprintln!("ERROR: {e}");
        exit(1);
    }
}

fn run(path: &str, show_symbols: bool) -> Result<(), String> {
    let out = Path::new(path).with_extension("bin");
    if out == Path::new(path) {
        return Err(format!("{path}: input already has a .bin extension"));
    }

    let src = std::fs::read_to_string(path).map_err(
        |e| format!("{path}: {e}")
    )?;
    let (bytes, symbols) = landfill::asm::assemble(&src)?;

    std::fs::write(&out, &bytes).map_err(|
        e| format!("{}: {e}", out.display())
    )?;

    if show_symbols {
        for (name, addr) in &symbols {
            println!("{name} {addr:#010x}");
        }
    }
    Ok(())
}
