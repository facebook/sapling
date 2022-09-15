/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Normalizes various path format on Windows. This function will convert
/// various Windows path format to full path form. Note this function does not
/// canonicalize the given path. So it does not collapse dots nor expand
/// relative paths.
/// Ref: https://googleprojectzero.blogspot.com/2016/02/the-definitive-guide-on-win32-to-nt.html
#[cfg(windows)]
fn normalize_windows_path(path: &str) -> String {
    let path = path.replace("/", r"\");
    if let Some(path) = path.strip_prefix(r"\??\UNC\") {
        // NT UNC path
        format!(r"\\{}", path)
    } else if let Some(path) = path.strip_prefix(r"\??\") {
        // NT path
        path.to_owned()
    } else if let Some(path) = path.strip_prefix(r"\\?\UNC\") {
        // Extend-length UNC path
        format!(r"\\{}", path)
    } else if let Some(path) = path.strip_prefix(r"\\?\") {
        // Extend-length path
        path.to_owned()
    } else {
        path
    }
}

/// Given a string representation of a path, encode it such that all
/// file/path special characters are replaced with non-special characters.
/// This has the effect of flattening a relative path fragment like
/// `foo/bar` into a single level path component like `fooZbar`.
/// Scratch uses this to give the appearance of hierarchy to clients
/// without having an actual hierarchy.  This is important on systems
/// such as Windows and macOS where the filesystem watchers are always
/// recursive.
/// The mapping is not and does not need to be reversible.
/// Why not just compute a SHA or MD5 hash?  It is nicer for the user
/// to have an idea of what the path is when they list the scratch container
/// path, which is something they'll likely end up doing when their disk
/// gets full, and they'll appreciate knowing which of these dirs have
/// value to them.
pub fn zzencode(path: &str) -> String {
    let mut result = String::with_capacity(path.len());

    // `std::fs::canonicalize` on Windows will normalize path into
    // extended-length format, which has a prefix `\\?\`. This function will
    // incorrect generate a path with the question mark which is an invalid
    // path.
    #[cfg(windows)]
    let path = &normalize_windows_path(path);

    for (i, b) in path.chars().enumerate() {
        if cfg!(unix) && i == 0 && b == '/' {
            // On unix, most paths begin with a slash, which
            // means that we'd use a Z prefix everything.
            // Let's just skip the first character.
            continue;
        }
        match b {
            '/' | '\\' => result.push('Z'),
            'Z' => result.push_str("_Z"),
            ':' => result.push_str("_"),
            _ => result.push(b),
        }
    }

    result
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_zzencode() {
        if cfg!(unix) {
            assert_eq!(zzencode("/foo/bar"), "fooZbar");
        } else {
            assert_eq!(zzencode("/foo/bar"), "ZfooZbar");
        }
        assert_eq!(zzencode("foo"), "foo");
        assert_eq!(zzencode("foo/bar"), "fooZbar");
        assert_eq!(zzencode(r"foo\bar"), "fooZbar");
        assert_eq!(zzencode("fooZbar"), "foo_Zbar");
        assert_eq!(zzencode("foo_Zbar"), "foo__Zbar");
        assert_eq!(zzencode(r"C:\foo\bar"), "C_ZfooZbar");
        assert_eq!(zzencode(r"\\unc\path"), "ZZuncZpath");

        if cfg!(windows) {
            assert_eq!(zzencode(r"\\?\C:\foo\bar"), "C_ZfooZbar");
            assert_eq!(zzencode(r"\\?\UNC\unc\path"), "ZZuncZpath");
            assert_eq!(zzencode(r"\??\C:\foo\bar"), "C_ZfooZbar");
            assert_eq!(zzencode(r"\??\UNC\unc\path"), "ZZuncZpath");
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_normalize_windows_path() {
        assert_eq!(normalize_windows_path(r"c:\foo\bar"), r"c:\foo\bar");
        assert_eq!(normalize_windows_path(r"c:/foo/bar"), r"c:\foo\bar");
        assert_eq!(normalize_windows_path(r"\??\c:\foo\bar"), r"c:\foo\bar");
        assert_eq!(normalize_windows_path(r"\\?\c:\foo\bar"), r"c:\foo\bar");
        assert_eq!(
            normalize_windows_path(r"\??\UNC\server\foo\bar"),
            r"\\server\foo\bar"
        );
        assert_eq!(
            normalize_windows_path(r"\\?\UNC\server\foo\bar"),
            r"\\server\foo\bar"
        );
    }
}
