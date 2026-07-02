fn main() {
    if let Err(err) = mdview::run() {
        eprintln!("mdview: {err}");
        std::process::exit(1);
    }
}
