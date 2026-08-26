fn main() {
    // Destructors must run (chat's executor closes memory in a Drop, on both
    // the success and the failure path of `run_chat`), so only failures exit
    // explicitly — with the classified code:
    // 0 success, 2 user error, 1 internal failure (`cli::Failure`).
    let code = mooshik::run();
    if code != 0 {
        std::process::exit(code);
    }
}
