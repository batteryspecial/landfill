use std::fs::File;
use std::io::{self, Read};
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

    // mmap for big files) and assemble
    let src = read(path).map_err(|e| format!("{path}: {e}"))?;
    let (bytes, symbols) = landfill::asm::assemble_bytes(src).ok_or_else(
        || format!("{path}: stream did not contain valid UTF-8")
    )??;
    
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

fn read(path: &str) -> io::Result<&'static [u8]> {
    let mut f = File::open(path)?;
    #[cfg(unix)]
    {
        let len = f.metadata()?.len() as usize;
        // only map files of 1MB or more (setup cost not worth it)
        if len >= 1 << 20 {
            if let Some(src) = map(&f, len) {
                return Ok(src);
            }
        }
    }
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer)?;
    Ok(buffer.leak())
}

#[cfg(unix)]
fn map(f: &File, len: usize) -> Option<&'static [u8]> {
    use std::ffi::{c_int, c_void};
    use std::os::fd::AsRawFd;

    unsafe extern "C" {
        fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void;
    }

    const PROT_READ: c_int = 1; // read-only
    const MAP_PRIV: c_int = 2; // private view

    // mmap redirectes file pointer to page-cache pages
    let p = unsafe { 
        mmap(std::ptr::null_mut(), len, PROT_READ, MAP_PRIV, f.as_raw_fd(), 0)
    };

    if p as isize == -1 {
        // mmap failed
        return None;
    }

    Some(unsafe {
        std::slice::from_raw_parts(p as *const u8, len)
    })
}

