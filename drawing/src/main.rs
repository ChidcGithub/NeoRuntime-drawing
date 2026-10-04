fn main() {
    if let Err(error) = board_app::run(board_app::AppMode::Drawing) {
        eprintln!("画板：{error}");
        std::process::exit(1);
    }
}
