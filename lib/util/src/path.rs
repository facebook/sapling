// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Path-related utilities.

use std::io;
use std::path::{Component, Path, PathBuf};

/// Normalize a canonicalized Path for display.
///
/// This removes the UNC prefix `\\?\` on Windows.
pub fn normalize_for_display(path: &str) -> &str {
    if cfg!(windows) && path.starts_with(r"\\?\") {
        &path[4..]
    } else {
        path
    }
}

/// Similar to [`normalize_for_display`]. But work on bytes.
pub fn normalize_for_display_bytes(path: &[u8]) -> &[u8] {
    if cfg!(windows) && path.starts_with(br"\\?\") {
        &path[4..]
    } else {
        path
    }
}

/// Return the absolute and normalized path without accessing the filesystem.
///
/// Unlike [`fs::canonicalize`], do not follow symlinks.
///
/// This function does not access the filesystem. Therefore it can behave
/// differently from the kernel or other library functions in corner cases.
/// For example:
///
/// - On some systems with symlink support, `foo/bar/..` and `foo` can be
///   different as seen by the kernel, if `foo/bar` is a symlink. This
///   function always returns `foo` in this case.
/// - On Windows, the official normalization rules are much more complicated.
///   See https://github.com/rust-lang/rust/pull/47363#issuecomment-357069527.
///   For example, this function cannot translate "drive relative" path like
///   "X:foo" to an absolute path.
///
/// Return an error if `std::env::current_dir()` fails or if this function
/// fails to produce an absolute path.
pub fn absolute(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = path.as_ref();
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("cannot get absoltue path from {:?}", path),
        ));
    }

    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {
                result.push(component);
            }
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => (),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    mod windows {
        use super::*;

        #[test]
        fn test_absolute_fullpath() {
            assert_eq!(absolute("C:/foo").unwrap(), Path::new("C:\\foo"));
            assert_eq!(
                absolute("x:\\a/b\\./.\\c").unwrap(),
                Path::new("x:\\a\\b\\c")
            );
            assert_eq!(
                absolute("y:/a/b\\../..\\c\\../d\\./.").unwrap(),
                Path::new("y:\\d")
            );
            assert_eq!(
                absolute("z:/a/b\\../..\\../..\\..").unwrap(),
                Path::new("z:\\")
            );
        }
    }

    #[cfg(unix)]
    mod unix {
        use super::*;

        #[test]
        fn test_absolute_fullpath() {
            assert_eq!(absolute("/a/./b\\c/../d/.").unwrap(), Path::new("/a/d"));
            assert_eq!(absolute("/a/../../../../b").unwrap(), Path::new("/b"));
            assert_eq!(absolute("/../../..").unwrap(), Path::new("/"));
            assert_eq!(absolute("/../../../").unwrap(), Path::new("/"));
            assert_eq!(
                absolute("//foo///bar//baz").unwrap(),
                Path::new("/foo/bar/baz")
            );
            assert_eq!(absolute("//").unwrap(), Path::new("/"));
        }
    }
}
