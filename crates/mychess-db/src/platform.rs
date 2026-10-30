//! Platform details kept apart (`.claude/rules/multiplataforma.md`).

use std::path::PathBuf;

/// Removes the Windows verbatim prefix that `fs::canonicalize` adds, so the
/// path reads as users and other programs write it: `\\?\C:\dir` becomes
/// `C:\dir` and `\\?\UNC\server\share` becomes `\\server\share`. Other paths
/// (and every path on other platforms) are returned unchanged.
pub(crate) fn display_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(text) = path.to_str()
            && let Some(plain) = strip_verbatim(text)
        {
            return PathBuf::from(plain);
        }
    }
    path
}

/// The text form of the rule of [`display_path`]; `None` when there is no
/// verbatim disk or UNC prefix to remove.
#[cfg_attr(not(windows), allow(dead_code))]
fn strip_verbatim(text: &str) -> Option<String> {
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{rest}"));
    }
    let rest = text.strip_prefix(r"\\?\")?;
    let mut chars = rest.chars();
    let drive = chars.next()?;
    (drive.is_ascii_alphabetic() && chars.next() == Some(':') && chars.next() == Some('\\'))
        .then(|| rest.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_verbatim_disk_and_unc_prefixes() {
        assert_eq!(
            strip_verbatim(r"\\?\C:\dir\x.pgn").as_deref(),
            Some(r"C:\dir\x.pgn")
        );
        assert_eq!(
            strip_verbatim(r"\\?\UNC\srv\share\x.pgn").as_deref(),
            Some(r"\\srv\share\x.pgn")
        );
    }

    #[test]
    fn leaves_other_paths_alone() {
        assert_eq!(strip_verbatim(r"C:\dir\x.pgn"), None);
        assert_eq!(strip_verbatim(r"\\?\Volume{1234}\x"), None);
        assert_eq!(strip_verbatim(r"\\?\C:"), None);
        assert_eq!(strip_verbatim("/home/u/x.pgn"), None);
    }
}
