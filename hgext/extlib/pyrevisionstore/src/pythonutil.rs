use cpython::{exc, PyBytes, PyErr, Python};
use failure::Error;
use revisionstore::key::Key;

pub fn to_pyerr(py: Python, error: &Error) -> PyErr {
    PyErr::new::<exc::KeyError, _>(py, format!("{}", error.cause()))
}

pub fn to_key(py: Python, name: &PyBytes, node: &PyBytes) -> Key {
    let mut bytes: [u8; 20] = Default::default();
    bytes.copy_from_slice(&node.data(py)[0..20]);
    Key::new(name.data(py).into(), (&bytes).into())
}
