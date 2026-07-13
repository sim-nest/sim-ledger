fn main() {
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let code = sim_ledger_cli::run(std::env::args().skip(1), &mut stdout, &mut stderr);
    std::process::exit(code);
}
