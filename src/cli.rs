use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run(PathBuf),
    Help,
    Version,
}

pub const HELP: &str = "Usage: mdview <file>\n\nA read-only terminal Markdown previewer.";

pub fn parse_args<I>(args: I) -> Result<CliAction, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let _program = args.next();
    let Some(first) = args.next() else {
        return Err(format!("{HELP}\n\nerror: missing file path"));
    };

    if first == "-h" || first == "--help" {
        return Ok(CliAction::Help);
    }

    if first == "-V" || first == "--version" {
        return Ok(CliAction::Version);
    }

    if let Some(extra) = args.next() {
        return Err(format!(
            "{HELP}\n\nerror: unexpected extra argument: {}",
            extra.to_string_lossy()
        ));
    }

    Ok(CliAction::Run(PathBuf::from(first)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_file_path() {
        assert_eq!(
            parse_args(os(&["mdview", "doc.md"])).unwrap(),
            CliAction::Run(PathBuf::from("doc.md"))
        );
    }

    #[test]
    fn parses_help_and_version() {
        assert_eq!(
            parse_args(os(&["mdview", "--help"])).unwrap(),
            CliAction::Help
        );
        assert_eq!(
            parse_args(os(&["mdview", "--version"])).unwrap(),
            CliAction::Version
        );
    }

    #[test]
    fn rejects_missing_and_extra_args() {
        assert!(parse_args(os(&["mdview"])).is_err());
        assert!(parse_args(os(&["mdview", "a.md", "b.md"])).is_err());
    }
}
