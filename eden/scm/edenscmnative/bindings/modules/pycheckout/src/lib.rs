/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use anyhow::format_err;
use async_runtime::block_on_exclusive as block_on;
use checkout::CheckoutPlan;
use cpython::*;
use cpython_ext::{ExtractInnerRef, PyNone, PyPathBuf, ResultPyErrExt};
use manifest_tree::Diff;
use pathmatcher::{AlwaysMatcher, Matcher};
use pymanifest::treemanifest;
use pypathmatcher::PythonMatcher;
use pyrevisionstore::contentstore;
use std::cell::RefCell;
use vfs::VFS;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "checkout"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<checkoutplan>(py)?;
    Ok(m)
}

py_class!(class checkoutplan |py| {
    data plan: RefCell<Option<CheckoutPlan>>;

    def __new__(
        _cls,
        current_manifest: &treemanifest,
        target_manifest: &treemanifest,
        matcher: Option<PyObject> = None,
    ) -> PyResult<checkoutplan> {
        let matcher: Box<dyn Matcher> = match matcher {
            None => Box::new(AlwaysMatcher::new()),
            Some(pyobj) => Box::new(PythonMatcher::new(py, pyobj)),
        };

        let current = current_manifest.borrow_underlying(py);
        let target = target_manifest.borrow_underlying(py);
        let diff = Diff::new(&current, &target, &matcher);
        let plan = CheckoutPlan::from_diff(diff).map_pyerr(py)?;
        checkoutplan::create_instance(py, RefCell::new(Some(plan)))
    }

    def apply(&self, root: PyPathBuf, content_store: &contentstore) -> PyResult<PyNone> {
        let vfs = VFS::new(root.to_path_buf()).map_pyerr(py)?;
        let store = content_store.extract_inner_ref(py);
        let plan = self.plan(py).borrow_mut()
            .take()
            .ok_or_else(|| format_err!("checkoutplan::apply can not be called twice on the same checkoutplan"))
            .map_pyerr(py)?;
        block_on(plan.apply_remote_data_store(&vfs, store)).map_pyerr(py)?;
        Ok(PyNone)
    }

    def __str__(&self) -> PyResult<PyString> {
        if let Some(plan) = self.plan(py).borrow().as_ref() {
            Ok(PyString::new(py, &plan.to_string()))
        } else {
            // Not returning error since ideally __str_ should not fail
            Ok(PyString::new(py, "checkoutplan was already applied"))
        }
    }

});
