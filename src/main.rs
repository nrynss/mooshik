fn main() {
    if let Err(err) = mooshik::run() {
        // One cause chain per line; no Rust type names in what the user sees.
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
