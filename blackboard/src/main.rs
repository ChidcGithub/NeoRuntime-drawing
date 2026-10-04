fn main() {
    if let Err(error) = board_app::run(board_app::AppMode::Blackboard) {
        eprintln!("黑板：{error}");
        std::process::exit(1);
    }
}
