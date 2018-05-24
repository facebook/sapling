#![deny(warnings)]

pub use failure::Error;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "unexpected error {}", _0)] ScmDaemonUnexpectedError(String),
}

pub type Result<T> = ::std::result::Result<T, Error>;
