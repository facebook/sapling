/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Path-related utilities.

use std::borrow::Cow;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::fs::remove_file as fs_remove_file;
use std::io;
use std::io::ErrorKind;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::errors::IOContext;
use crate::errors::IOResult;

/// Pick a random file name `path.$RAND.atomic` as `real_path`. Write `data` to
/// it.  Then modify the symlink `path` to point to `real_path`.  Attempt to
/// delete files that are no longer referred.
///
/// Since the symlink itself cannot be mmap-ed on Windows, this function is
/// suitable for large mmap buffer on Windows. Without a symlink the mmap
/// file has to be removed first, otherwise it cannot be replaced.
///
/// Unlike `tempfile::NamedTempFile`, this function does not `chmod` the file.
///
/// This function has a side effect of creating a `path.lock` file for
/// locking.
///
/// Attention: the deletion attempt is based on file name. So do not use
/// confusing file names like `path.0001.atomic` in the same directory.
pub fn atomic_write_symlink(path: &Path, data: &[u8]) -> IOResult<()> {
    let append_name = |suffix: &str| -> PathBuf {
        let mut s = path.to_path_buf().into_os_string();
        s.push(suffix);
        s.into()
    };
    let temp_name = || -> PathBuf { append_name(&format!(".{:x}.atomic", rand::random::<u32>())) };

    // Protect racy write operations by a lock.
    let _lock = crate::lock::PathLock::exclusive(&append_name(".lock"))?;

    // Pick a name. Open the file.
    let (real_path, mut file) = loop {
        let real_path = temp_name();
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&real_path)
        {
            Ok(file) => break Ok((real_path, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue, // try another file name
            Err(e) => {
                break Err(e).path_context("error opening atomic symlink real path", &real_path);
            }
        }
    }?;
    let real_file_name = real_path
        .file_name()
        .expect("real_path should have a file name");

    // Write the content.
    file.write_all(data)
        .path_context("error writing atomic symlink data to real path", &real_path)?;
    drop(file);

    // Update the symlink by creating a temporary symlink and rename it.
    let symlink_tmp_path = loop {
        let symlink_path = temp_name();
        match symlink_file(Path::new(real_file_name), &symlink_path) {
            Ok(()) => break Ok(symlink_path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue, // try another file name
            Err(e) => {
                break Err(e).path_context("error creating temp atomic symlink", &symlink_path);
            }
        }
    }?;

    // Overwrite the original symlink. This works on both Windows and Linux.
    fs::rename(&symlink_tmp_path, path).path_context("error renaming temp atomic symlink", path)?;

    // Scan. Remove unreferenced files.
    let _ = (|| -> IOResult<()> {
        let looks_like_atomic = |s: &OsStr, prefix: &OsStr| -> bool {
            if let (Some(s), Some(prefix)) = (s.to_str(), prefix.to_str()) {
                s.starts_with(prefix) && s.ends_with(".atomic")
            } else {
                false
            }
        };
        if let (Some(dir), Some(prefix)) = (path.parent(), path.file_name()) {
            for entry in fs::read_dir(dir).path_context("error reading atomic symlink dir", dir)? {
                let entry = entry.path_context("error reading atomic symlink dir entry", dir)?;
                let name = entry.file_name();
                if name != prefix && looks_like_atomic(&name, prefix) && name != real_file_name {
                    let _ = remove_file(&entry.path());
                }
            }
        }
        Ok(())
    })();

    Ok(())
}

/// Create symlink for a file.
pub fn symlink_file(src: &Path, dst: &Path) -> io::Result<()> {
    #[cfg(windows)]
    return std::os::windows::fs::symlink_file(src, dst);
    #[cfg(unix)]
    return std::os::unix::fs::symlink(src, dst);
    #[cfg(all(not(unix), not(windows)))]
    return Err(io::Error::new(
        ErrorKind::Other,
        "symlink is not supported by the system",
    ));
}

/// Removes the UNC prefix `\\?\` on Windows. Does nothing on unices.
pub fn strip_unc_prefix(path: &Path) -> &Path {
    path.strip_prefix(r"\\?\").unwrap_or(path)
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
            format!("cannot get absolute path from {:?}", path),
        ));
    }

    Ok(normalize(&path))
}

/// Normalize path to collapse "..", ".", and duplicate separators. This
/// function does not access the filesystem, so it can return an
/// incorrect result if the path contains symlinks.
///
///     # use std::path::Path;
///     # use util::path::normalize;
///     assert_eq!(normalize("foo/.//bar/../baz/".as_ref()), Path::new("foo/baz"));
///
///     // Interesting edge cases:
///     assert_eq!(normalize("".as_ref()), Path::new("."));
///     assert_eq!(normalize("..".as_ref()), Path::new(".."));
///     assert_eq!(normalize("/..".as_ref()), Path::new("/"));
///
/// This behavior matches that of Python's `os.path.normpath` and Go's `path.Clean`.
pub fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    let mut poppable: usize = 0;
    let mut has_root = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => {
                poppable += 1;
                result.push(component);
            }
            Component::RootDir => {
                has_root = true;
                result.push(component);
            }
            Component::Prefix(_) => {
                result.push(component);
            }
            Component::ParentDir => {
                if poppable > 0 {
                    result.pop();
                    poppable -= 1;
                } else if !has_root {
                    result.push(component);
                }
            }
            Component::CurDir => {}
        }
    }

    if result.as_os_str().is_empty() {
        return ".".into();
    }

    result
}

/// Remove the file pointed by `path`.
pub fn remove_file<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();
    // On Windows, try to rename the file before removing.
    // This allows re-creating a same file.
    // See https://boostgsoc13.github.io/boost.afio/doc/html/afio/FAQ/deleting_open_files.html
    let path: Cow<Path> = if cfg!(windows) {
        let tmp_path = path.with_extension(format!("tmp.{:x}", rand::random::<u16>()));
        fs::rename(path, &tmp_path)?;
        Cow::Owned(tmp_path)
    } else {
        Cow::Borrowed(path)
    };
    let result = fs_remove_file(&path);
    #[cfg(windows)]
    match &result {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            // This file might be mmapp-ed. Try a different way.
            if windows_remove_mmap_file(&path).is_ok() {
                return Ok(());
            }
        }
        _ => {}
    }
    result.map_err(Into::into)
}

/// Deletes a file even if it is being mmap-ed on Windows.
/// See https://stackoverflow.com/questions/54138684.
#[cfg(windows)]
fn windows_remove_mmap_file(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use winapi::shared::minwindef::FALSE;
    use winapi::um::fileapi::CreateFileW;
    use winapi::um::fileapi::OPEN_EXISTING;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::winbase::FILE_FLAG_DELETE_ON_CLOSE;
    use winapi::um::winnt::DELETE;
    use winapi::um::winnt::FILE_SHARE_DELETE;
    use winapi::um::winnt::FILE_SHARE_READ;
    use winapi::um::winnt::FILE_SHARE_WRITE;

    let wpath: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wpath.as_ptr(),
            DELETE,
            FILE_SHARE_DELETE | FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_DELETE_ON_CLOSE,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(err)
        }
    } else if unsafe { CloseHandle(handle) } == FALSE {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Create the directory with specified permission on UNIX systems. We create a temporary
/// directory at the parent directory of the the directory being created, run chmod to change the
/// permission then rename the temporary directory to the desired name to prevent leaking directory
/// with incorrect permissions.
#[cfg(unix)]
fn create_dir_with_mode_impl(path: &Path, mode: u32) -> io::Result<()> {
    if path.exists() {
        if path.is_dir() {
            // If metadata operation fails, it's fine.
            if let Ok(metadata) = path.metadata() {
                if metadata.permissions().mode() & mode != mode {
                    // We only attempt to fix the permission. If we can't, proceed.
                    // TODO: We should at least generate a warning here. We cannot because we
                    // cannot print messages in Mercurial Rust yet.
                    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
                }
            }
        }

        return Err(io::ErrorKind::AlreadyExists.into());
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::NotFound,
            format!("`{:?}` does not have a parent directory", path),
        )
    })?;
    let temp = tempfile::TempDir::new_in(&parent)?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(mode))?;

    let temp = temp.into_path();
    if let Err(e) = fs::rename(&temp, path) {
        // In the unlikely event where the rename fails, we attempt to clean up the
        // previously leaked temporary file before returning.
        let _ = fs::remove_dir(&temp);

        // The rename may fail if the desinated directory already exists and is not empty. In this
        // case it will return `ENOTEMPTY` instead of `EEXIST`. Rust does not have an
        // `io::ErrorKind` for such error, and it will be categorized into `ErrorKind::Other`. We
        // have to use `libc::ENOTEMPTY` because the integer value of `ENOTEMPTY` varies depends on
        // platform we are on.
        // Similarly, when the destinated directory is a file, we get `ENOTDIR` instead of `EEXIST`.
        match e.raw_os_error() {
            Some(libc::ENOTEMPTY) | Some(libc::ENOTDIR) => Err(ErrorKind::AlreadyExists.into()),
            _ => Err(e),
        }
    } else {
        Ok(())
    }
}

/// Create the directory. The mode argument is ignored on non-UNIX systems.
#[cfg(not(unix))]
fn create_dir_with_mode_impl(path: &Path, _mode: u32) -> io::Result<()> {
    fs::create_dir(path)
}

fn create_dir_with_mode(path: &Path, mode: u32) -> io::Result<()> {
    match create_dir_with_mode_impl(path, mode) {
        Ok(()) => Ok(()),
        Err(e) => {
            if e.kind() == ErrorKind::AlreadyExists && path.is_dir() {
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

/// Create the directory and ignore failures when a directory of the same name already exists.
pub fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
    create_dir_with_mode(path.as_ref(), 0o755)
}

/// Create the directory with group write permission on UNIX systems.
pub fn create_shared_dir(path: impl AsRef<Path>) -> io::Result<()> {
    create_dir_with_mode(path.as_ref(), 0o2775)
}

/// Create the directory and its ancestors. The mode argument is ignored on non-UNIX systems.
pub fn create_dir_all_with_mode(path: impl AsRef<Path>, mode: u32) -> io::Result<()> {
    let mut to_create = vec![path.as_ref()];
    while let Some(dir) = to_create.pop() {
        match create_dir_with_mode(dir, mode) {
            Ok(()) => continue,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                to_create.push(dir);
                match dir.parent() {
                    Some(parent) => to_create.push(parent),
                    None => return Err(err),
                }
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Create the directory and ancestors with group write permission on UNIX systems.
pub fn create_shared_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    create_dir_all_with_mode(path, 0o2775)
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

/// Return a relative [`PathBuf`] to the path from the base path.
pub fn relativize(base: &Path, path: &Path) -> PathBuf {
    let mut base_iter = base.iter();
    let mut path_iter = path.iter();
    let mut rel_path = PathBuf::new();
    loop {
        match (base_iter.next(), path_iter.next()) {
            (Some(ref c), Some(ref p)) if c == p => continue,

            // Examples:
            // b: foo/bar/baz/
            // p: foo/bar/biz/buzz.html
            // This is the common case where we have to go up some number of directories
            // (so one "../" per unique path component of base) and then back down.
            //
            // b: foo/bar/baz/biz/
            // p: foo/bar/
            // If foo/bar was a file and then the user replaced it with a directory, and now
            // the user is in a subdirectory of that directory, then one "../" per unique path
            // component of base.
            (Some(_c), remaining_path) => {
                // Find the common prefix of path and base. Prefix with one "../" per unique
                // path component of base and then append the unique sequence of components from
                // path.
                rel_path.push(".."); // This is for the current component, c.
                for _ in base_iter {
                    rel_path.push("..");
                }

                if let Some(p) = remaining_path {
                    rel_path.push(p);
                    for component in path_iter {
                        rel_path.push(component);
                    }
                }
                break;
            }

            // Example:
            // b: foo/bar/
            // p: foo/bar/baz/buzz.html
            (None, Some(p)) => {
                rel_path.push(p);
                for component in path_iter {
                    rel_path.push(component);
                }
                break;
            }

            // Example:
            // b: foo/bar/baz/
            // p: foo/bar/baz/
            // If foo/bar/baz was a file and then the user replaced it with a directory, which
            // is also the user's current directory, then "" should be returned.
            (None, None) => {
                break;
            }
        }
    }

    rel_path
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use anyhow::Result;
    use tempfile::TempDir;

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

        #[test]
        fn test_normalize_path() {
            assert_eq!(normalize(r"a/b\c\..\.".as_ref()), Path::new(r"a\b"));
            assert_eq!(normalize("z:/a//b/./".as_ref()), Path::new(r"z:\a\b"));
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

        #[test]
        fn test_normalize_path() {
            assert_eq!(normalize("/a/./b/../d/.".as_ref()), Path::new("/a/d"));
            assert_eq!(normalize("./a/b/c/../../".as_ref()), Path::new("a"));
            assert_eq!(normalize("".as_ref()), Path::new("."));
            assert_eq!(normalize(".".as_ref()), Path::new("."));
            assert_eq!(normalize("..".as_ref()), Path::new(".."));
            assert_eq!(normalize("/..".as_ref()), Path::new("/"));
            assert_eq!(normalize("/../..".as_ref()), Path::new("/"));
            assert_eq!(normalize("./..".as_ref()), Path::new(".."));
            assert_eq!(normalize("../../..".as_ref()), Path::new("../../.."));
            assert_eq!(normalize("////".as_ref()), Path::new("/"));
        }

        #[test]
        fn test_create_dir_mode() -> Result<()> {
            let tempdir = TempDir::new()?;
            let mut path = tempdir.path().to_path_buf();
            path.push("dir");
            create_dir(&path)?;
            assert!(path.is_dir());
            let metadata = path.metadata()?;
            assert_eq!(metadata.permissions().mode(), 0o40755);
            // check we don't have temporary directory left
            assert_eq!(tempdir.path().read_dir()?.count(), 1);
            Ok(())
        }

        #[test]
        fn test_create_shared_dir() -> Result<()> {
            let tempdir = TempDir::new()?;
            let mut path = tempdir.path().to_path_buf();
            path.push("shared");
            create_shared_dir(&path)?;
            assert!(path.is_dir());
            let metadata = path.metadata()?;
            assert_eq!(metadata.permissions().mode(), 0o42775);
            // check we don't have temporary directory left
            assert_eq!(tempdir.path().read_dir()?.count(), 1);
            Ok(())
        }

        #[test]
        fn test_fixup_perms() -> Result<()> {
            let tempdir = TempDir::new()?;
            let mut path = tempdir.path().to_path_buf();
            path.push("shared");

            // Create it without the SGID bit.
            create_dir_with_mode(&path, 0o775)?;
            let metadata = path.metadata()?;
            assert_eq!(metadata.permissions().mode(), 0o40775);

            // Fix it up.
            create_shared_dir(&path)?;
            let metadata = path.metadata()?;
            assert_eq!(metadata.permissions().mode(), 0o42775);

            Ok(())
        }
    }

    fn test_create_dir_all_fn(
        create_fn: &dyn Fn(&PathBuf) -> io::Result<()>,
        mode: u32,
    ) -> Result<()> {
        let tempdir = TempDir::new()?;
        let path = tempdir.path().join("foo").join("bar");
        create_fn(&path)?;
        assert!(path.is_dir());

        #[cfg(unix)]
        {
            let metadata = path.metadata()?;
            assert_eq!(metadata.permissions().mode(), mode);
        }

        Ok(())
    }

    #[test]
    fn test_atomic_write_symlink() -> Result<()> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let j = |name| -> PathBuf { path.join(name) };
        // a: unrelated file
        fs::write(j("a"), b"1")?;
        // c: symlink -> c.xxxx: "2"
        atomic_write_symlink(&j("c"), b"2")?;
        // Keep an mmap of the current "c".
        let file = fs::OpenOptions::new().read(true).open(&j("c"))?;
        let mmap = unsafe { memmap::Mmap::map(&file) }?;
        // This file should be automatically deleted.
        fs::write(j("c.aaaa.atomic"), b"0")?;
        // Rewrite c with different data.
        atomic_write_symlink(&j("c"), b"3")?;
        // mmap has the old content.
        assert_eq!(mmap.as_ref(), b"2");
        // Reading c gets new data.
        assert_eq!(fs::read(&j("c"))?, b"3");
        // a: should exist
        assert!(j("a").exists());
        // 4 files: a, c, c.xxxx, c.lock
        let count = || {
            fs::read_dir(&path)
                .unwrap()
                .filter(|e| e.as_ref().unwrap().path().exists())
                .count()
        };
        assert_eq!(count(), 4);

        // It's possible to replace a non-symlink to a symlink.
        // (but this might fail if "a" is mmap-ed on Windows).
        atomic_write_symlink(&j("a"), b"4")?;
        // Exercise the GC logic a bit.
        atomic_write_symlink(&j("a"), b"5")?;
        atomic_write_symlink(&j("a"), b"6")?;
        // 6 files: a, a.xxxx, a.lock, c, c.xxxx, c.lock
        assert_eq!(count(), 6);
        Ok(())
    }

    #[test]
    fn test_mmap_delete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foo");
        fs::write(&path, b"bar").unwrap();
        let file = fs::OpenOptions::new().read(true).open(&path).unwrap();
        let _mmap = unsafe { memmap::Mmap::map(&file) }.unwrap();
        remove_file(&path).unwrap();
        assert!(!path.exists());
        // The file can be recreated in-place.
        fs::write(&path, b"baz").unwrap();
    }

    #[test]
    fn test_create_dir_non_exist() -> Result<()> {
        let tempdir = TempDir::new()?;
        let mut path = tempdir.path().to_path_buf();
        path.push("dir");
        create_dir(&path)?;
        assert!(path.is_dir());
        Ok(())
    }

    #[test]
    fn test_create_dir_exist() -> Result<()> {
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
    fn test_create_dir_file_exist() -> Result<()> {
        let tempdir = TempDir::new()?;
        let mut path = tempdir.path().to_path_buf();
        path.push("dir");
        File::create(&path)?;
        let err = create_dir(&path).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        Ok(())
    }

    #[test]
    fn test_create_dir_with_nonexistent_parent() -> Result<()> {
        let tempdir = TempDir::new()?;
        let mut path = tempdir.path().to_path_buf();
        path.push("nonexistentparent");
        path.push("dir");
        let err = create_dir(&path).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        Ok(())
    }

    #[test]
    fn test_create_dir_without_empty_path() {
        let empty = Path::new("");
        let err = create_dir(&empty).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
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

    #[test]
    fn test_create_shared_dir_all() -> Result<()> {
        test_create_dir_all_fn(&|path| create_shared_dir_all(path), 0o42775)
    }

    #[test]
    fn test_create_dir_all_with_mode() -> Result<()> {
        test_create_dir_all_fn(&|path| create_dir_all_with_mode(path, 0o777), 0o40777)
    }

    #[test]
    fn test_relativize_absolute_paths() {
        let check = |base, path, expected| {
            assert_eq!(
                relativize(Path::new(base), Path::new(path)),
                Path::new(expected)
            );
        };
        check("/", "/", "");
        check("/foo/bar/baz", "/foo/bar/baz", "");
        check("/foo/bar", "/foo/bar/baz", "baz");
        check("/foo", "/foo/bar/baz", "bar/baz");
        check("/foo/bar/baz", "/foo/bar", "..");
        check("/foo/bar/baz", "/foo", "../..");
        check("/foo/bar/baz", "/foo/BAR", "../../BAR");
        check("/foo/bar/baz", "/foo/BAR/BAZ", "../../BAR/BAZ");
    }

    #[test]
    fn test_relativize_platform_absolute_paths() {
        // This test with Windows-style absolute paths on Windows, and Unix-style path on Unix
        let cwd = Path::new(".").canonicalize().unwrap();
        let result = relativize(&cwd, &cwd.join("a").join("b"));
        assert_eq!(result, Path::new("a").join("b"));
    }
}
