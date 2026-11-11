//! Windows command line parser for startup entries and Run/RunOnce keys.
//!
//! Separates an executable path from its command-line arguments while respecting
//! Windows quoting rules and whitespace boundaries.

/// Represents a parsed command line separated into target executable and arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommandLine {
    pub target: String,
    pub args: Option<String>,
}

/// Parses a Windows command-line string (such as found in registry Run/RunOnce keys)
/// into an executable path and its arguments.
pub fn parse_command_line(cmd: &str) -> Option<ParsedCommandLine> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(after_quote) = trimmed.strip_prefix('"') {
        // Quoted executable: find closing quote
        if let Some(end_quote) = after_quote.find('"') {
            let target = after_quote[..end_quote].trim().to_string();
            let remaining = after_quote[end_quote + 1..].trim();
            let args = if remaining.is_empty() {
                None
            } else {
                Some(remaining.to_string())
            };
            Some(ParsedCommandLine { target, args })
        } else {
            // Unmatched opening quote: treat rest as target
            Some(ParsedCommandLine {
                target: after_quote.trim().to_string(),
                args: None,
            })
        }
    } else {
        // Unquoted executable: check for .exe boundary or first space
        let lower = trimmed.to_lowercase();
        if let Some(exe_pos) = lower.find(".exe") {
            let exe_end = exe_pos + 4;
            let target = trimmed[..exe_end].trim().to_string();
            let remaining = trimmed[exe_end..].trim();
            let args = if remaining.is_empty() {
                None
            } else {
                Some(remaining.to_string())
            };
            Some(ParsedCommandLine { target, args })
        } else if let Some(space_pos) = trimmed.find(' ') {
            let target = trimmed[..space_pos].trim().to_string();
            let remaining = trimmed[space_pos..].trim();
            let args = if remaining.is_empty() {
                None
            } else {
                Some(remaining.to_string())
            };
            Some(ParsedCommandLine { target, args })
        } else {
            // Single token
            Some(ParsedCommandLine {
                target: trimmed.to_string(),
                args: None,
            })
        }
    }
}

/// Inspects RunOnce entry prefix:
/// `!` = defer deletion until command completes successfully
/// `*` = run in safe mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOncePrefix {
    pub defer_deletion: bool,
    pub allow_safe_mode: bool,
}

pub fn parse_runonce_prefix(name: &str) -> (RunOncePrefix, &str) {
    let mut defer_deletion = false;
    let mut allow_safe_mode = false;
    let mut current = name;

    loop {
        if let Some(stripped) = current.strip_prefix('!') {
            defer_deletion = true;
            current = stripped;
        } else if let Some(stripped) = current.strip_prefix('*') {
            allow_safe_mode = true;
            current = stripped;
        } else {
            break;
        }
    }

    (
        RunOncePrefix {
            defer_deletion,
            allow_safe_mode,
        },
        current,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_command_with_args() {
        let parsed = parse_command_line(r#""C:\Program Files\Vendor\App.exe" --background /flag"#).unwrap();
        assert_eq!(parsed.target, r#"C:\Program Files\Vendor\App.exe"#);
        assert_eq!(parsed.args.as_deref(), Some("--background /flag"));
    }

    #[test]
    fn parses_quoted_command_without_args() {
        let parsed = parse_command_line(r#""C:\Program Files\Vendor\App.exe""#).unwrap();
        assert_eq!(parsed.target, r#"C:\Program Files\Vendor\App.exe"#);
        assert_eq!(parsed.args, None);
    }

    #[test]
    fn parses_unquoted_exe_with_args() {
        let parsed = parse_command_line(r#"C:\Windows\System32\cmd.exe /c echo hi"#).unwrap();
        assert_eq!(parsed.target, r#"C:\Windows\System32\cmd.exe"#);
        assert_eq!(parsed.args.as_deref(), Some("/c echo hi"));
    }

    #[test]
    fn parses_unquoted_simple_command() {
        let parsed = parse_command_line(r#"calc.exe"#).unwrap();
        assert_eq!(parsed.target, "calc.exe");
        assert_eq!(parsed.args, None);
    }

    #[test]
    fn parses_runonce_prefixes() {
        let (prefix, clean) = parse_runonce_prefix("!MyUpdateTask");
        assert!(prefix.defer_deletion);
        assert!(!prefix.allow_safe_mode);
        assert_eq!(clean, "MyUpdateTask");

        let (prefix, clean) = parse_runonce_prefix("*!ImportantTask");
        assert!(prefix.defer_deletion);
        assert!(prefix.allow_safe_mode);
        assert_eq!(clean, "ImportantTask");

        let (prefix, clean) = parse_runonce_prefix("NormalTask");
        assert!(!prefix.defer_deletion);
        assert!(!prefix.allow_safe_mode);
        assert_eq!(clean, "NormalTask");
    }
}
