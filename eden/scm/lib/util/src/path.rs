/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Path-related utilities.

use std::env;
#[cfg(not(unix))]
use std::fs::rename;
use std::fs::{self, remove_file as fs_remove_file};
use std::io::{self, ErrorKind};
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

/// Create the directory and ignore failures when a directory of the same name already exists.
pub fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
    match fs::create_dir(path.as_ref()) {
        Ok(()) => Ok(()),
        Err(e) => {
            if e.kind() == ErrorKind::AlreadyExists && path.as_ref().is_dir() {
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

/// Expand the user's home directory and any environment variables references in
/// the given path.
///
/// This function is designed to emulate the behavior of Mercurial's `util.expandpath`
/// function, which in turn uses Python's `os.path.expand{user,vars}` functions. This
/// results in behavior that is notably different from the default expansion behavior
/// of the `shellexpand` crate. In particular:
///
/// - If a reference to an environment variable is missing or invalid, the reference
///   is left unchanged in the resulting path rather than emitting an error.
///
/// - Home directory expansion explicitly happens after environment variable
///   expansion, meaning that if an environment variable is expanded into a
///   string starting with a tilde (`~`), the tilde will be expanded into the
///   user's home directory.
///
pub fn expand_path(path: impl AsRef<str>) -> PathBuf {
    expand_path_impl(path.as_ref(), |k| env::var(k).ok(), dirs::home_dir)
}

/// Same as `expand_path` but explicitly takes closures for environment variable
/// and home directory lookup for the sake of testability.
fn expand_path_impl<E, H>(path: &str, getenv: E, homedir: H) -> PathBuf
where
    E: FnMut(&str) -> Option<String>,
    H: FnOnce() -> Option<PathBuf>,
{
    // The shellexpand crate does not expand Windows environment variables
    // like `%PROGRAMDATA%`. We'd like to expand them too. So let's do some
    // pre-processing.
    //
    // XXX: Doing this preprocessing has the unfortunate side-effect that
    // if an environment variable fails to expand on Windows, the resulting
    // string will contain a UNIX-style environment variable reference.
    //
    // e.g., "/foo/%MISSING%/bar" will expand to "/foo/${MISSING}/bar"
    //
    // The current approach is good enough for now, but likely needs to
    // be improved later for correctness.
    let path = {
        let mut new_path = String::new();
        let mut is_starting = true;
        for ch in path.chars() {
            if ch == '%' {
                if is_starting {
                    new_path.push_str("${");
                } else {
                    new_path.push('}');
                }
                is_starting = !is_starting;
            } else if cfg!(windows) && ch == '/' {
                // Only on Windows, change "/" to "\" automatically.
                // This makes sure "%include /foo" works as expected.
                new_path.push('\\')
            } else {
                new_path.push(ch);
            }
        }
        new_path
    };

    let path = shellexpand::env_with_context_no_errors(&path, getenv);
    shellexpand::tilde_with_context(&path, homedir)
        .as_ref()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::File;

    use tempfile::TempDir;

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

    #[test]
    fn test_create_dir_non_exist() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut path = tempdir.path().to_path_buf();
        path.push("dir");
        create_dir(&path)?;
        assert!(path.is_dir());
        Ok(())
    }

    #[test]
    fn test_create_dir_exist() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut path = tempdir.path().to_path_buf();
        path.push("dir");
        create_dir(&path)?;
        assert!(&path.is_dir());
        create_dir(&path)?;
        assert!(&path.is_dir());
        Ok(())
    }

    #[test]
    fn test_create_dir_file_exist() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut path = tempdir.path().to_path_buf();
        path.push("dir");
        File::create(&path)?;
        let err = create_dir(&path).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        Ok(())
    }

    #[test]
    fn test_path_expansion() {
        fn getenv(key: &str) -> Option<String> {
            match key {
                "foo" => Some("~/a".into()),
                "bar" => Some("b".into()),
                _ => None,
            }
        }

        fn homedir() -> Option<PathBuf> {
            Some(PathBuf::from("/home/user"))
        }

        let path = "$foo/${bar}/$baz";
        let expected = PathBuf::from("/home/user/a/b/$baz");

        assert_eq!(expand_path_impl(&path, getenv, homedir), expected);
    }
}
