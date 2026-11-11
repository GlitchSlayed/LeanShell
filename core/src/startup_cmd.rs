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
        // Unquoted executable: search for ".exe" (ASCII-only, case-insensitive) followed by
        // end-of-string, whitespace, or a flag character. Crucially we find the position in
        // the ORIGINAL string — never apply an offset from a `.to_lowercase()` result, because
        // Unicode lowercase can change byte length (e.g. 'İ' → "i\u{307}").
        //
        // We also require that the character immediately preceding ".exe" is NOT a path
        // separator, to avoid splitting on directory components like "C:\tools.exe-cache\app.exe".
        let exe_suffix = b".exe";
        let bytes = trimmed.as_bytes();
        let mut found_exe_end: Option<usize> = None;
        for start in 0..bytes.len().saturating_sub(3) {
            if bytes[start..start + 4].eq_ignore_ascii_case(exe_suffix) {
                let end = start + 4;
                // The char after ".exe" must be end-of-string, whitespace, or a switch char.
                let next_ok = end >= bytes.len() || bytes[end] == b' ' || bytes[end] == b'\t';
                // The char before ".exe" must not be a path separator (avoid dir components).
                let prev_ok = start == 0 || (bytes[start - 1] != b'\\' && bytes[start - 1] != b'/');
                if next_ok && prev_ok {
                    found_exe_end = Some(end);
                    break;
                }
            }
        }

        if let Some(exe_end) = found_exe_end {
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

    /// U06 regression: .exe in a directory component must not split the path there.
    /// "C:\tools.exe-cache\app.exe --flag" should produce target="C:\tools.exe-cache\app.exe".
    #[test]
    fn exe_in_directory_name_uses_last_exe() {
        let parsed = parse_command_line(r"C:\tools.exe-cache\app.exe --flag").unwrap();
        assert_eq!(parsed.target, r"C:\tools.exe-cache\app.exe");
        assert_eq!(parsed.args.as_deref(), Some("--flag"));
    }

    /// U06 regression: .exe immediately followed by a path separator is a dir component, not an exe.
    #[test]
    fn exe_followed_by_separator_is_not_target() {
        // "C:\foo.exe\bar.exe" — the first .exe is a dir, the second is the target.
        let parsed = parse_command_line(r"C:\foo.exe\bar.exe").unwrap();
        assert_eq!(parsed.target, r"C:\foo.exe\bar.exe");
        assert_eq!(parsed.args, None);
    }

    /// U06 regression: Unicode command-line must never panic.
    /// A string containing a multi-byte char (Turkish 'İ' lowercases to two bytes) must
    /// parse without panicking.
    #[test]
    fn unicode_command_does_not_panic() {
        // This would panic with the old to_lowercase() byte-offset approach
        let result = parse_command_line(r"C:\İ.exe --arg");
        // We don't assert exact output since OS path handling varies, just that it doesn't panic.
        let _ = result;

        // Also check with emoji in path (multi-byte UTF-8)
        let result2 = parse_command_line(r"C:\App 🚀\launcher.exe --start");
        if let Some(parsed) = result2 {
            assert!(parsed.target.ends_with("launcher.exe"));
        }
    }
}
