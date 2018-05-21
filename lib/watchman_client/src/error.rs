#![deny(warnings)]

pub use failure::Error;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(
        display = "[{}] watchman command line transport request failed\n[{} error] {}", _0, _0, _1
    )]
    CommandLineTransportError(&'static str, String),

    #[fail(
        display = "[{}] watchman unix socket transport request failed\n[{} error] {}", _0, _0, _1
    )]
    UnixSocketTransportError(&'static str, String),

    #[fail(
        display = "[{}] watchman windows named pipe transport request failed\n[{} error] {}",
        _0,
        _0,
        _1
    )]
    WindowsNamedPipeTransportError(&'static str, String),

    #[fail(display = "watchman bser protocol parsing error {}", _0)]
    WatchmanBserParsingError(String),

    #[fail(display = "error while decoding watchman pdu {}", _0)]
    WatchmanError(String),
}

pub type Result<T> = ::std::result::Result<T, Error>;
