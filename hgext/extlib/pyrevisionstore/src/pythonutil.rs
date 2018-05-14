use cpython::{exc, FromPyObject, PyBytes, PyErr, PyObject, PyResult, PyTuple, Python,
              PythonObject, ToPyObject};
use failure::Error;
use revisionstore::datastore::Delta;
use revisionstore::key::Key;

pub fn to_pyerr(py: Python, error: &Error) -> PyErr {
    PyErr::new::<exc::KeyError, _>(py, format!("{}", error.cause()))
}

pub fn to_key(py: Python, name: &PyBytes, node: &PyBytes) -> Key {
    let mut bytes: [u8; 20] = Default::default();
    bytes.copy_from_slice(&node.data(py)[0..20]);
    Key::new(name.data(py).into(), (&bytes).into())
}

pub fn from_key(py: Python, key: &Key) -> (PyBytes, PyBytes) {
    (
        PyBytes::new(py, key.name()),
        PyBytes::new(py, key.node().as_ref()),
    )
}

pub fn from_tuple_to_delta<'a>(py: Python, py_delta: &PyObject) -> PyResult<Delta> {
    // A python delta is a tuple: (name, node, base name, base node, delta bytes)
    let py_delta = PyTuple::extract(py, &py_delta)?;
    let py_name = PyBytes::extract(py, &py_delta.get_item(py, 0))?;
    let py_node = PyBytes::extract(py, &py_delta.get_item(py, 1))?;
    let py_delta_name = PyBytes::extract(py, &py_delta.get_item(py, 2))?;
    let py_delta_node = PyBytes::extract(py, &py_delta.get_item(py, 3))?;
    let py_bytes = PyBytes::extract(py, &py_delta.get_item(py, 4))?;

    Ok(Delta {
        data: py_bytes.data(py).to_vec().into_boxed_slice(),
        base: to_key(py, &py_delta_name, &py_delta_node),
        key: to_key(py, &py_name, &py_node),
    })
}

pub fn from_delta_to_tuple(py: Python, delta: &Delta) -> PyObject {
    let (name, node) = from_key(py, &delta.key);
    let (base_name, base_node) = from_key(py, &delta.base);
    let bytes = PyBytes::new(py, &delta.data);
    // A python delta is a tuple: (name, node, base name, base node, delta bytes)
    (
        name.into_object(),
        node.into_object(),
        base_name.into_object(),
        base_node.into_object(),
        bytes.into_object(),
    ).into_py_object(py)
        .into_object()
}

pub fn from_key_to_tuple<'a>(py: Python, key: &'a Key) -> PyTuple {
    let (py_name, py_node) = from_key(py, key);
    PyTuple::new(py, &[py_name.into_object(), py_node.into_object()])
}

pub fn from_tuple_to_key(py: Python, py_tuple: &PyObject) -> PyResult<Key> {
    let py_tuple = <&PyTuple>::extract(py, &py_tuple)?.as_slice(py);
    let name = <&PyBytes>::extract(py, &py_tuple[0])?;
    let node = <&PyBytes>::extract(py, &py_tuple[1])?;
    Ok(to_key(py, &name, &node))
}
