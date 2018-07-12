use std::io;
use std::path::PathBuf;

/// The error type for parsing config files.
#[derive(Fail, Debug)]
pub enum Error {
    // TODO: use line number instead of byte offsets.
    /// Unable to parse a file due to syntax or encoding error in the file content.
    #[fail(display = "{:?}: parse error around byte {}: {}", _0, _1, _2)]
    Parse(PathBuf, usize, &'static str),

    /// Unable to read a file due to IO errors.
    #[fail(display = "{:?}: {}", _0, _1)]
    Io(PathBuf, #[cause] io::Error),
}
