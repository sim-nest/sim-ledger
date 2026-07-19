#![forbid(unsafe_code)]

mod orphans;
mod simdoc;

fn main() {
    if let Err(err) = run(std::env::args().collect()) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    match args.get(1).map(String::as_str) {
        Some("simdoc") => simdoc::run(args),
        Some("check-orphan-crates") => orphans::run(),
        _ => Err(format!(
            "usage: {program} simdoc [--check]\n       {program} check-orphan-crates"
        )),
    }
}
