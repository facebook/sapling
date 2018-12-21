extern crate cpython;
extern crate python27_sys;

mod bytearrayobject;
mod bytesobject;

pub use bytearrayobject::{boxed_slice_to_pyobj, vec_to_pyobj};
pub use bytesobject::allocate_pybytes;
