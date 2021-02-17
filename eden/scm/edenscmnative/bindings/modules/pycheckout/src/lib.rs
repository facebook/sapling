/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use checkout::CheckoutPlan;
use cpython::*;
use cpython_ext::ResultPyErrExt;
use manifest_tree::Diff;
use pathmatcher::{AlwaysMatcher, Matcher};
use pymanifest::treemanifest;
use pypathmatcher::PythonMatcher;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "checkout"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<checkoutplan>(py)?;
    Ok(m)
}

py_class!(class checkoutplan |py| {
    data plan: CheckoutPlan;

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
        checkoutplan::create_instance(py, plan)
    }

});
