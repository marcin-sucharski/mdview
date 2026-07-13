use crate::rendered::{Segment, Style};
use crate::syntax::highlight_code;
use crate::watcher::{absolute_path, is_reload_event_kind};
use notify::{RecursiveMode, Watcher};
use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_PORT: u16 = 7312;
const DEFAULT_LISTEN: &str = "127.0.0.1";
const MAX_REQUEST_LINE: usize = 16 * 1024;
const MAX_MARKDOWN_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RAW_BYTES: u64 = 64 * 1024 * 1024;
const CHANGE_DEBOUNCE: Duration = Duration::from_millis(100);
const SSE_HEARTBEAT: Duration = Duration::from_secs(15);
const APP_CSP: &str = "default-src 'self'; img-src 'self' data: http: https:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'";
const RAW_SVG_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

pub const HELP: &str = r#"Usage: mdview-web [OPTIONS] [PATH]

Serve a live Markdown workspace in a browser. PATH defaults to the current
directory. If PATH is a Markdown file, its parent is the workspace and the
file is opened initially.

Options:
  -p, --port <PORT>       HTTP port (default: 7312)
      --listen <ADDRESS>  Listen address or host (default: 127.0.0.1)
  -I, --all-interfaces    Listen on all IPv4 interfaces (0.0.0.0)
  -h, --help              Print help
  -V, --version           Print version"#;

#[derive(Debug)]
pub enum WebError {
    Cli(String),
    Io(io::Error),
    Notify(notify::Error),
}

impl fmt::Display for WebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Notify(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for WebError {}

impl From<io::Error> for WebError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<notify::Error> for WebError {
    fn from(value: notify::Error) -> Self {
        Self::Notify(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebConfig {
    pub path: PathBuf,
    pub listen: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebAction {
    Run(WebConfig),
    Help,
    Version,
}

#[derive(Debug)]
struct Workspace {
    root: PathBuf,
    initial: Option<String>,
}

#[derive(Debug, Default)]
struct EventHub {
    clients: Mutex<Vec<SyncSender<u64>>>,
}

pub fn run() -> Result<(), WebError> {
    match parse_args(env::args_os()).map_err(WebError::Cli)? {
        WebAction::Help => {
            println!("{HELP}");
            Ok(())
        }
        WebAction::Version => {
            println!("mdview-web {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        WebAction::Run(config) => serve(config),
    }
}

pub fn parse_args<I>(args: I) -> Result<WebAction, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let _program = args.next();
    let mut path: Option<PathBuf> = None;
    let mut listen: Option<String> = None;
    let mut port = DEFAULT_PORT;
    let mut all_interfaces = false;
    let mut positional_only = false;

    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        if !positional_only {
            match text.as_ref() {
                "-h" | "--help" => return Ok(WebAction::Help),
                "-V" | "--version" => return Ok(WebAction::Version),
                "--" => {
                    positional_only = true;
                    continue;
                }
                "-I" | "--all-interfaces" => {
                    all_interfaces = true;
                    continue;
                }
                "-p" | "--port" => {
                    let value = args
                        .next()
                        .ok_or_else(|| cli_error("missing value for --port"))?;
                    port = parse_port(&value)?;
                    continue;
                }
                "--listen" => {
                    let value = args
                        .next()
                        .ok_or_else(|| cli_error("missing value for --listen"))?;
                    listen = Some(parse_listen(&value)?);
                    continue;
                }
                _ => {}
            }

            if let Some(value) = text.strip_prefix("--port=") {
                port = parse_port(&OsString::from(value))?;
                continue;
            }
            if let Some(value) = text.strip_prefix("--listen=") {
                listen = Some(parse_listen(&OsString::from(value))?);
                continue;
            }
            if text.starts_with('-') {
                return Err(cli_error(&format!("unknown option: {text}")));
            }
        }

        if path.replace(PathBuf::from(arg)).is_some() {
            return Err(cli_error("unexpected extra path argument"));
        }
    }

    if all_interfaces && listen.is_some() {
        return Err(cli_error(
            "--all-interfaces cannot be combined with --listen",
        ));
    }

    Ok(WebAction::Run(WebConfig {
        path: path.unwrap_or_else(|| PathBuf::from(".")),
        listen: if all_interfaces {
            "0.0.0.0".to_string()
        } else {
            listen.unwrap_or_else(|| DEFAULT_LISTEN.to_string())
        },
        port,
    }))
}

fn cli_error(message: &str) -> String {
    format!("{HELP}\n\nerror: {message}")
}

fn parse_port(value: &OsString) -> Result<u16, String> {
    value
        .to_string_lossy()
        .parse::<u16>()
        .map_err(|_| cli_error(&format!("invalid port: {}", value.to_string_lossy())))
}

fn parse_listen(value: &OsString) -> Result<String, String> {
    let value = value.to_string_lossy();
    if value.trim().is_empty() {
        Err(cli_error("listen address cannot be empty"))
    } else {
        Ok(value.into_owned())
    }
}

fn serve(config: WebConfig) -> Result<(), WebError> {
    let workspace = Arc::new(Workspace::discover(&config.path)?);
    let listener = TcpListener::bind((config.listen.as_str(), config.port))?;
    let local = listener.local_addr()?;
    let hub = Arc::new(EventHub::default());
    let (change_tx, change_rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            if is_reload_event_kind(event.kind) {
                let _ = change_tx.send(());
            }
        }
    })?;
    watcher.watch(&workspace.root, RecursiveMode::Recursive)?;
    spawn_change_dispatcher(change_rx, Arc::clone(&hub));

    println!("mdview-web serving {}", workspace.root.display());
    println!("mdview-web listening on http://{local}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let workspace = Arc::clone(&workspace);
                let hub = Arc::clone(&hub);
                thread::spawn(move || {
                    if let Err(err) = handle_connection(stream, &workspace, &hub) {
                        eprintln!("mdview-web: request error: {err}");
                    }
                });
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(WebError::Io(err)),
        }
    }

    drop(watcher);
    Ok(())
}

impl Workspace {
    fn discover(path: &Path) -> io::Result<Self> {
        let path = absolute_path(path);
        let metadata = fs::metadata(&path)?;
        if metadata.is_dir() {
            return Ok(Self {
                root: fs::canonicalize(path)?,
                initial: None,
            });
        }
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a file or directory", path.display()),
            ));
        }

        let file = fs::canonicalize(path)?;
        let root = file
            .parent()
            .ok_or_else(|| io::Error::other("file has no parent directory"))?
            .to_path_buf();
        let initial = file
            .strip_prefix(&root)
            .ok()
            .and_then(path_to_web)
            .ok_or_else(|| io::Error::other("file name is not valid UTF-8"))?;
        Ok(Self {
            root,
            initial: Some(initial),
        })
    }

    fn requested_path(&self, encoded: &str) -> Result<(String, PathBuf), RequestError> {
        let decoded = percent_decode(encoded)?;
        let relative = Path::new(&decoded);
        if decoded.is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err(RequestError::bad_request("invalid workspace path"));
        }
        let path = self.root.join(relative);
        Ok((decoded, path))
    }

    fn checked_existing_path(&self, encoded: &str) -> Result<(String, PathBuf), RequestError> {
        let (relative, path) = self.requested_path(encoded)?;
        let canonical = fs::canonicalize(&path).map_err(RequestError::from_io)?;
        if !canonical.starts_with(&self.root) {
            return Err(RequestError::forbidden("path leaves the workspace"));
        }
        Ok((relative, canonical))
    }
}

fn spawn_change_dispatcher(rx: Receiver<()>, hub: Arc<EventHub>) {
    thread::spawn(move || {
        let mut revision = 0u64;
        while rx.recv().is_ok() {
            while rx.recv_timeout(CHANGE_DEBOUNCE).is_ok() {}
            revision = revision.wrapping_add(1);
            hub.broadcast(revision);
        }
    });
}

impl EventHub {
    fn subscribe(&self) -> Receiver<u64> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.clients.lock().expect("event hub poisoned").push(tx);
        rx
    }

    fn broadcast(&self, revision: u64) {
        self.clients
            .lock()
            .expect("event hub poisoned")
            .retain(|client| match client.try_send(revision) {
                Ok(()) | Err(TrySendError::Full(_)) => true,
                Err(TrySendError::Disconnected(_)) => false,
            });
    }
}

#[derive(Debug)]
struct RequestError {
    status: u16,
    message: String,
}

impl RequestError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: 403,
            message: message.into(),
        }
    }

    fn from_io(err: io::Error) -> Self {
        Self {
            status: if err.kind() == io::ErrorKind::NotFound {
                404
            } else {
                500
            },
            message: err.to_string(),
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    workspace: &Workspace,
    hub: &EventHub,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let target = match read_request_target(&stream) {
        Ok(target) => target,
        Err(err) => return write_json_error(&mut stream, err.status, &err.message),
    };
    let (path, query) = target
        .split_once('?')
        .map_or((target.as_str(), ""), |(path, query)| (path, query));

    match path {
        "/" | "/open" => serve_app(&mut stream, workspace),
        "/api/tree" => serve_tree(&mut stream, workspace),
        "/api/file" => serve_markdown(&mut stream, workspace, query),
        "/raw" => serve_raw(&mut stream, workspace, query),
        "/events" => serve_events(stream, hub),
        _ => write_json_error(&mut stream, 404, "not found"),
    }
}

fn read_request_target(stream: &TcpStream) -> Result<String, RequestError> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .by_ref()
        .take(MAX_REQUEST_LINE as u64)
        .read_line(&mut line)
        .map_err(RequestError::from_io)?;
    if line.len() >= MAX_REQUEST_LINE {
        return Err(RequestError::bad_request("request line is too long"));
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if method != "GET" {
        return Err(RequestError {
            status: 405,
            message: "only GET is supported".to_string(),
        });
    }
    if target.is_empty() || !version.starts_with("HTTP/") {
        return Err(RequestError::bad_request("malformed HTTP request"));
    }
    Ok(target.to_string())
}

fn serve_app(stream: &mut TcpStream, workspace: &Workspace) -> io::Result<()> {
    let initial = workspace
        .initial
        .as_deref()
        .map(json_string)
        .unwrap_or_else(|| "null".to_string());
    let root = json_string(&workspace.root.display().to_string());
    let body = include_str!("web_app.html")
        .replace("__MDVIEW_INITIAL__", &initial)
        .replace("__MDVIEW_ROOT__", &root);
    write_response(stream, 200, "text/html; charset=utf-8", body.as_bytes())
}

fn serve_tree(stream: &mut TcpStream, workspace: &Workspace) -> io::Result<()> {
    let mut files = Vec::new();
    collect_markdown_files(&workspace.root, &workspace.root, &mut files);
    files.sort_by_key(|path| path.to_lowercase());
    let body = format!(
        "{{\"files\":[{}]}}",
        files
            .iter()
            .map(|path| json_string(path))
            .collect::<Vec<_>>()
            .join(",")
    );
    write_response(
        stream,
        200,
        "application/json; charset=utf-8",
        body.as_bytes(),
    )
}

fn collect_markdown_files(root: &Path, directory: &Path, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), ".git" | ".direnv" | "target") {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_markdown_files(root, &path, files);
        } else if file_type.is_file() && is_markdown_path(&path) {
            if let Ok(relative) = path.strip_prefix(root) {
                if let Some(relative) = path_to_web(relative) {
                    files.push(relative);
                }
            }
        }
    }
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd"
            )
        })
}

fn serve_markdown(stream: &mut TcpStream, workspace: &Workspace, query: &str) -> io::Result<()> {
    let encoded = match query_parameter(query, "path") {
        Some(value) => value,
        None => return write_json_error(stream, 400, "missing path query parameter"),
    };
    let (relative, path) = match workspace.requested_path(encoded) {
        Ok(value) => value,
        Err(err) => return write_json_error(stream, err.status, &err.message),
    };
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let body = format!(
                "{{\"path\":{},\"deleted\":true,\"fingerprint\":null,\"html\":\"\"}}",
                json_string(&relative)
            );
            return write_response(
                stream,
                410,
                "application/json; charset=utf-8",
                body.as_bytes(),
            );
        }
        Err(err) => return write_json_error(stream, 500, &err.to_string()),
    };
    if !metadata.is_file() {
        return write_json_error(stream, 400, "path is not a file");
    }
    if metadata.len() > MAX_MARKDOWN_BYTES {
        return write_json_error(stream, 413, "Markdown file is too large");
    }
    let canonical = match fs::canonicalize(&path) {
        Ok(path) if path.starts_with(&workspace.root) => path,
        Ok(_) => return write_json_error(stream, 403, "path leaves the workspace"),
        Err(err) => return write_json_error(stream, 500, &err.to_string()),
    };
    let source = match fs::read_to_string(&canonical) {
        Ok(source) => source,
        Err(err) if err.kind() == io::ErrorKind::InvalidData => {
            return write_json_error(stream, 422, "Markdown file is not valid UTF-8")
        }
        Err(err) => return write_json_error(stream, 500, &err.to_string()),
    };
    let rendered = render_markdown_html(workspace, Path::new(&relative), &source);
    let fingerprint = source_fingerprint(&source);
    let body = format!(
        "{{\"path\":{},\"deleted\":false,\"fingerprint\":{},\"html\":{}}}",
        json_string(&relative),
        json_string(&fingerprint),
        json_string(&rendered)
    );
    write_response(
        stream,
        200,
        "application/json; charset=utf-8",
        body.as_bytes(),
    )
}

fn source_fingerprint(source: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn serve_raw(stream: &mut TcpStream, workspace: &Workspace, query: &str) -> io::Result<()> {
    let encoded = match query_parameter(query, "path") {
        Some(value) => value,
        None => return write_json_error(stream, 400, "missing path query parameter"),
    };
    let (_, path) = match workspace.checked_existing_path(encoded) {
        Ok(value) => value,
        Err(err) => return write_json_error(stream, err.status, &err.message),
    };
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(err) => return write_json_error(stream, 404, &err.to_string()),
    };
    if !metadata.is_file() {
        return write_json_error(stream, 400, "path is not a file");
    }
    if metadata.len() > MAX_RAW_BYTES {
        return write_json_error(stream, 413, "file is too large");
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => return write_json_error(stream, 500, &err.to_string()),
    };
    let content_type = content_type(&path);
    if content_type == "image/svg+xml" {
        write_response_with_csp(stream, 200, content_type, &bytes, RAW_SVG_CSP)
    } else {
        write_response(stream, 200, content_type, &bytes)
    }
}

fn serve_events(mut stream: TcpStream, hub: &EventHub) -> io::Result<()> {
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache, no-store\r\nConnection: keep-alive\r\nX-Content-Type-Options: nosniff\r\n\r\nevent: ready\ndata: connected\n\n",
    )?;
    stream.flush()?;
    let events = hub.subscribe();
    loop {
        match events.recv_timeout(SSE_HEARTBEAT) {
            Ok(revision) => write!(stream, "event: change\ndata: {revision}\n\n")?,
            Err(mpsc::RecvTimeoutError::Timeout) => stream.write_all(b": keepalive\n\n")?,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
        stream.flush()?;
    }
}

fn write_json_error(stream: &mut TcpStream, status: u16, message: &str) -> io::Result<()> {
    let body = format!("{{\"error\":{}}}", json_string(message));
    write_response(
        stream,
        status,
        "application/json; charset=utf-8",
        body.as_bytes(),
    )
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    write_response_with_csp(stream, status, content_type, body, APP_CSP)
}

fn write_response_with_csp(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    content_security_policy: &str,
) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: {content_security_policy}\r\nConnection: close\r\n\r\n",
        status_reason(status),
        body.len()
    )?;
    stream.write_all(body)
}

fn status_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        413 => "Content Too Large",
        422 => "Unprocessable Content",
        _ => "Internal Server Error",
    }
}

fn query_parameter<'a>(query: &'a str, wanted: &str) -> Option<&'a str> {
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        (name == wanted).then_some(value)
    })
}

fn percent_decode(value: &str) -> Result<String, RequestError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex_value(bytes[index + 1]);
                let low = hex_value(bytes[index + 2]);
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(RequestError::bad_request("invalid percent encoding"));
                };
                output.push((high << 4) | low);
                index += 3;
            }
            b'%' => return Err(RequestError::bad_request("invalid percent encoding")),
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).map_err(|_| RequestError::bad_request("path is not valid UTF-8"))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '<' => output.push_str("\\u003c"),
            '>' => output.push_str("\\u003e"),
            '&' => output.push_str("\\u0026"),
            '\u{2028}' => output.push_str("\\u2028"),
            '\u{2029}' => output.push_str("\\u2029"),
            ch if ch < '\u{20}' => output.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => output.push(ch),
        }
    }
    output.push('"');
    output
}

fn path_to_web(path: &Path) -> Option<String> {
    let parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "txt" | "md" | "markdown" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn render_markdown_html(workspace: &Workspace, markdown_path: &Path, source: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(source, options);
    let mut events = Vec::new();
    let mut code_block: Option<(String, String)> = None;

    for event in parser {
        if let Some((language, code)) = &mut code_block {
            match event {
                Event::End(TagEnd::CodeBlock) => {
                    events.push(Event::Html(CowStr::from(highlighted_code_html(
                        language, code,
                    ))));
                    code_block = None;
                }
                Event::Text(text) | Event::Code(text) => code.push_str(&text),
                Event::SoftBreak | Event::HardBreak => code.push('\n'),
                _ => {}
            }
            continue;
        }

        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Indented => String::new(),
                    CodeBlockKind::Fenced(language) => language.into_string(),
                };
                code_block = Some((language, String::new()));
            }
            Event::Start(tag) => {
                events.push(Event::Start(rewrite_tag(workspace, markdown_path, tag)))
            }
            Event::Html(raw) | Event::InlineHtml(raw) => events.push(Event::Text(raw)),
            event => events.push(event),
        }
    }
    if let Some((language, code)) = code_block {
        events.push(Event::Html(CowStr::from(highlighted_code_html(
            &language, &code,
        ))));
    }

    let mut output = String::new();
    html::push_html(&mut output, events.into_iter());
    output
}

fn rewrite_tag<'a>(workspace: &Workspace, markdown_path: &Path, tag: Tag<'a>) -> Tag<'a> {
    match tag {
        Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        } => Tag::Image {
            link_type,
            dest_url: CowStr::from(rewrite_destination(
                workspace,
                markdown_path,
                &dest_url,
                true,
            )),
            title,
            id,
        },
        Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        } => Tag::Link {
            link_type,
            dest_url: CowStr::from(rewrite_destination(
                workspace,
                markdown_path,
                &dest_url,
                false,
            )),
            title,
            id,
        },
        tag => tag,
    }
}

fn rewrite_destination(
    workspace: &Workspace,
    markdown_path: &Path,
    destination: &str,
    image: bool,
) -> String {
    if destination.is_empty() || destination.starts_with('#') || destination.starts_with("//") {
        return destination.to_string();
    }
    if let Some(scheme) = uri_scheme(destination) {
        return if matches!(scheme.as_str(), "http" | "https" | "mailto")
            || (image && scheme == "data")
        {
            destination.to_string()
        } else {
            "#".to_string()
        };
    }

    let path_part = destination.split(['?', '#']).next().unwrap_or(destination);
    let decoded = percent_decode(path_part)
        .map_err(|_| ())
        .unwrap_or_else(|_| path_part.to_string());
    let target = if Path::new(&decoded).is_absolute() {
        let absolute = Path::new(&decoded);
        absolute
            .strip_prefix(&workspace.root)
            .ok()
            .map(PathBuf::from)
    } else {
        normalize_relative(
            markdown_path.parent().unwrap_or_else(|| Path::new("")),
            &decoded,
        )
    };
    let Some(target) = target.and_then(|path| path_to_web(&path)) else {
        return "#".to_string();
    };
    let encoded = percent_encode(&target);
    if !image && is_markdown_path(Path::new(&target)) {
        format!("/open?path={encoded}")
    } else {
        format!("/raw?path={encoded}")
    }
}

fn uri_scheme(value: &str) -> Option<String> {
    let colon = value.find(':')?;
    let before = &value[..colon];
    (!before.is_empty()
        && before.chars().enumerate().all(|(index, ch)| {
            ch.is_ascii_alphanumeric() || (index > 0 && matches!(ch, '+' | '-' | '.'))
        }))
    .then(|| before.to_ascii_lowercase())
}

fn normalize_relative(base: &Path, destination: &str) -> Option<PathBuf> {
    let mut parts = base
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_os_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for component in Path::new(destination).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    let mut result = PathBuf::new();
    for part in parts {
        result.push(part);
    }
    Some(result)
}

fn highlighted_code_html(language: &str, source: &str) -> String {
    let language_class = language
        .split(|ch: char| ch.is_ascii_whitespace() || ch == '{' || ch == ',')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>();
    let mut output = format!(
        "<pre><code class=\"language-{}\">",
        escape_html(&language_class)
    );
    for (line_index, line) in highlight_code(language, source).iter().enumerate() {
        if line_index > 0 {
            output.push('\n');
        }
        for segment in line {
            output.push_str("<span class=\"");
            output.push_str(code_style_class(segment));
            output.push_str("\">");
            output.push_str(&escape_html(&segment.text));
            output.push_str("</span>");
        }
    }
    output.push_str("</code></pre>\n");
    output
}

fn code_style_class(segment: &Segment) -> &'static str {
    if segment.style == Style::code_keyword() {
        "tok-keyword"
    } else if segment.style == Style::code_key() {
        "tok-key"
    } else if segment.style == Style::code_string() {
        "tok-string"
    } else if segment.style == Style::code_number() {
        "tok-number"
    } else if segment.style == Style::code_literal() {
        "tok-literal"
    } else if segment.style == Style::code_punctuation() {
        "tok-punctuation"
    } else if segment.style == Style::code_comment() {
        "tok-comment"
    } else {
        "tok-plain"
    }
}

fn escape_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            ch => output.push(ch),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn test_workspace() -> Workspace {
        Workspace {
            root: PathBuf::from("/tmp/workspace"),
            initial: None,
        }
    }

    #[test]
    fn parses_defaults_and_network_options() {
        assert_eq!(
            parse_args(os(&["mdview-web"])).unwrap(),
            WebAction::Run(WebConfig {
                path: PathBuf::from("."),
                listen: "127.0.0.1".to_string(),
                port: 7312,
            })
        );
        assert_eq!(
            parse_args(os(&[
                "mdview-web",
                "--port",
                "8123",
                "--listen",
                "localhost",
                "docs"
            ]))
            .unwrap(),
            WebAction::Run(WebConfig {
                path: PathBuf::from("docs"),
                listen: "localhost".to_string(),
                port: 8123,
            })
        );
        assert_eq!(
            parse_args(os(&["mdview-web", "-I", "doc.md"])).unwrap(),
            WebAction::Run(WebConfig {
                path: PathBuf::from("doc.md"),
                listen: "0.0.0.0".to_string(),
                port: 7312,
            })
        );
    }

    #[test]
    fn rejects_conflicts_bad_ports_and_extra_paths() {
        assert!(parse_args(os(&["mdview-web", "--all-interfaces", "--listen", "::1"])).is_err());
        assert!(parse_args(os(&["mdview-web", "--all"])).is_err());
        assert!(parse_args(os(&["mdview-web", "--port", "70000"])).is_err());
        assert!(parse_args(os(&["mdview-web", "one", "two"])).is_err());
    }

    #[test]
    fn renders_tables_highlighting_safe_html_and_workspace_links() {
        let workspace = test_workspace();
        let source = "# Demo\n\n| A | B |\n| - | - |\n| x | y |\n\n```json\n{\"ok\": true}\n```\n\n![pic](assets/a.png)\n\n[next](next.md)\n\n[bad](javascript:alert(1))\n\n<script>alert(1)</script>";
        let html = render_markdown_html(&workspace, Path::new("docs/readme.md"), source);
        assert!(html.contains("<table>"));
        assert!(html.contains("tok-key"));
        assert!(html.contains("tok-literal"));
        assert!(html.contains("/raw?path=docs/assets/a.png"));
        assert!(html.contains("/open?path=docs/next.md"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn path_validation_blocks_traversal_and_decodes_unicode() {
        let workspace = test_workspace();
        assert!(workspace.requested_path("../secret.md").is_err());
        assert!(workspace.requested_path("/etc/passwd").is_err());
        let (relative, path) = workspace
            .requested_path("docs/%E6%97%A5%E6%9C%AC.md")
            .unwrap();
        assert_eq!(relative, "docs/日本.md");
        assert_eq!(path, PathBuf::from("/tmp/workspace/docs/日本.md"));
    }

    #[test]
    fn source_fingerprint_is_stable_and_changes_with_markdown() {
        assert_eq!(source_fingerprint("same"), source_fingerprint("same"));
        assert_ne!(source_fingerprint("before"), source_fingerprint("after"));
        assert_ne!(
            source_fingerprint("# Heading"),
            source_fingerprint("# Heading\n")
        );
    }

    #[test]
    fn tree_lists_only_markdown_and_skips_build_and_symlink_directories() {
        let dir = env::temp_dir().join(format!(
            "mdview-web-tree-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("guide")).unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::write(dir.join("README.md"), "# Root").unwrap();
        fs::write(dir.join("guide/start.markdown"), "# Start").unwrap();
        fs::write(dir.join("guide/no.txt"), "no").unwrap();
        fs::write(dir.join("target/ignored.md"), "no").unwrap();

        let mut files = Vec::new();
        collect_markdown_files(&dir, &dir, &mut files);
        files.sort();
        assert_eq!(files, vec!["README.md", "guide/start.markdown"]);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn web_app_has_scrollable_full_width_atomic_preview_layout() {
        let app = include_str!("web_app.html");
        assert!(app.contains("height: 100dvh"));
        assert!(app.contains("overflow-y: auto"));
        assert!(app.contains("padding: clamp(30px, 4.5vw, 68px)"));
        assert!(app.contains("async function prepareMarkdownPreview"));
        assert!(app.contains("previewElement.replaceChildren(...staging.childNodes)"));
        assert!(app.contains("generation !== previewGeneration"));
        assert!(app.contains("function cancelPendingPreview()"));
        assert!(app.contains("events.addEventListener(\"ready\", () =>"));
        assert!(app.contains("scheduleRefresh();"));
        assert!(app.contains(".tab.changed:not(.active)"));
        assert!(app.contains("changed since last view"));
        assert!(app.contains("if (sourceChanged && selected !== path) tab.changed = true"));
    }

    #[test]
    fn raw_svg_policy_blocks_active_content() {
        assert_eq!(content_type(Path::new("diagram.svg")), "image/svg+xml");
        assert!(RAW_SVG_CSP.contains("default-src 'none'"));
        assert!(RAW_SVG_CSP.contains("sandbox"));
        assert!(!RAW_SVG_CSP.contains("script-src 'unsafe-inline'"));
    }
}
