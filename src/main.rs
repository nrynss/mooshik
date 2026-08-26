fn main() {
    // Destructors must run on the success path (chat's executor closes memory
    // in a Drop), so only failures exit explicitly — with the classified code:
    // 0 success, 2 user error, 1 internal failure (`cli::Failure`).
    let code = mooshik::run();
    if code != 0 {
        std::process::exit(code);
    }
}
