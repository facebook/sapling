// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Path-related utilities.

use std::fs::remove_file as fs_remove_file;
#[cfg(not(unix))]
use std::fs::rename;
use std::io;
use std::path::{Component, Path, PathBuf};

use failure::Fallible;
#[cfg(not(unix))]
use tempfile::Builder;

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

/// Remove the file pointed by `path`.
#[cfg(unix)]
pub fn remove_file<P: AsRef<Path>>(path: P) -> Fallible<()> {
    fs_remove_file(path)?;
    Ok(())
}

/// Remove the file pointed by `path`.
///
/// On Windows, removing a file can fail for various reasons, including if the file is memory
/// mapped. This can happen when the repository is accessed concurrently while a background task is
/// trying to remove a packfile. To solve this, we can rename the file before trying to remove it.
/// If the remove operation fails, a future repack will clean it up.
#[cfg(not(unix))]
pub fn remove_file<P: AsRef<Path>>(path: P) -> Fallible<()> {
    let path = path.as_ref();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map_or(".to-delete".to_owned(), |ext| ".".to_owned() + ext + "-tmp");

    let dest_path = Builder::new()
        .prefix("")
        .suffix(&extension)
        .rand_bytes(8)
        .tempfile_in(path.parent().unwrap())?
        .into_temp_path();

    rename(path, &dest_path)?;

    // Ignore errors when removing the file, it will be cleaned up at a later time.
    let _ = fs_remove_file(dest_path);
    Ok(())
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
