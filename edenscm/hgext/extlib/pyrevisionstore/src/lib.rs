// Copyright Facebook, Inc. 2018
//! pyrevisionstore - Python interop layer for a Mercurial data and history store

#[macro_use]
extern crate cpython;

mod datastorepyext;
mod historystorepyext;
mod pyerror;
mod pythondatastore;
mod pythonhistorystore;
mod pythonutil;
mod repackablepyext;

#[allow(non_camel_case_types)]
pub mod pyext;

pub use crate::pyext::init_module;
pub use crate::pythondatastore::PythonMutableDataPack;
pub use crate::pythonhistorystore::PythonMutableHistoryPack;
