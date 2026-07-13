fn main() {
    if let Err(err) = mdview::web::run() {
        eprintln!("mdview-web: {err}");
        std::process::exit(1);
    }
}
