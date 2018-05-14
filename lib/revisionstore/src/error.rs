use failure::Error;
use std;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Fail)]
#[fail(display = "Key Error: {:?}", _0)]
pub struct KeyError(#[fail(cause)] Error);

impl KeyError {
    pub fn new(err: Error) -> Self {
        KeyError(err)
    }
}
