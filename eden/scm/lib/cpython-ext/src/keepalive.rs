/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use python3_sys as ffi;
use tracing::info_span;

/// Keep the Python interpreter alive.
///
/// Dropping all instances of this triggers `Py_Finalize`,
/// if `enable_py_finalize_on_drop` is called at least once.
pub struct PythonKeepAlive;

static REF_COUNT: AtomicUsize = AtomicUsize::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);

impl PythonKeepAlive {
    pub fn new() -> Self {
        REF_COUNT.fetch_add(1, Ordering::Release);
        Self
    }

    /// Enable calling `Py_Finalize` on dropping all `PythonKeepAlive`s.
    /// This is useful when the Python lifetime is explicitly maintained by us.
    /// Practically, `cmdpy::HgPython` might set this to `true`.
    pub fn enable_py_finalize_on_drop(self, value: bool) -> Self {
        ENABLED.store(value, Ordering::Release);
        self
    }
}

impl Drop for PythonKeepAlive {
    fn drop(&mut self) {
        let count = REF_COUNT.fetch_sub(1, Ordering::AcqRel);
        if count == 1 {
            maybe_py_finalize();
        }
    }
}

fn maybe_py_finalize() {
    unsafe {
        // Stop other threads from running Python logic.
        //
        // During Py_Finalize, other Python threads might pthread_exit and funny things might
        // happen with rust-cpython's unwind protection.  Example SIGABRT stacks:
        //
        // Thread 1 (exiting):
        // #0  ... in raise () from /lib64/libc.so.6
        // #1  ... in abort () from /lib64/libc.so.6
        // #2  ... in __libc_message () from /lib64/libc.so.6
        // #3  ... in __libc_fatal () from /lib64/libc.so.6
        // #4  ... in unwind_cleanup () from /lib64/libpthread.so.0
        // #5  ... in panic_unwind::real_imp::cleanup ()
        //     at library/panic_unwind/src/gcc.rs:78
        // #6  panic_unwind::__rust_panic_cleanup ()
        //     at library/panic_unwind/src/lib.rs:100
        // #7  ... in std::panicking::try::cleanup ()
        //     at library/std/src/panicking.rs:360
        // #8  ... in std::panicking::try::do_catch (data=<optimized out>, payload=... "\000")
        //     at library/std/src/panicking.rs:404
        // #9  std::panicking::try (f=...) at library/std/src/panicking.rs:343
        // #10 ... in std::panic::catch_unwind (f=...)
        //     at library/std/src/panic.rs:396
        // #11 cpython::function::handle_callback (_location=..., _c=..., f=...)
        //     at cpython-0.5.1/src/function.rs:216
        // #12 ... in pythreading::RGeneratorIter::create_instance::TYPE_OBJECT::wrap_unary (slf=...)
        //     at cpython-0.5.1/src/py_class/slots.rs:318
        // #13 ... in builtin_next () from /lib64/libpython3.6m.so.1.0
        // #14 ... in call_function () from /lib64/libpython3.6m.so.1.0
        // #15 ... in _PyEval_EvalFrameDefault () from /lib64/libpython3.6m.so.1.0
        // ....
        // #32 ... in PyObject_Call () from /lib64/libpython3.6m.so.1.0
        // #33 ... in t_bootstrap () from /lib64/libpython3.6m.so.1.0
        // #34 ... in pythread_wrapper () from /lib64/libpython3.6m.so.1.0
        // #35 ... in start_thread () from /lib64/libpthread.so.0
        // #36 ... in clone () from /lib64/libc.so.6
        //
        // Thread 2:
        // #0  ... in _int_free () from /lib64/libc.so.6
        // #1  ... in code_dealloc () from /lib64/libpython3.6m.so.1.0
        // #2  ... in func_dealloc () from /lib64/libpython3.6m.so.1.0
        // #3  ... in PyObject_ClearWeakRefs () from /lib64/libpython3.6m.so.1.0
        // #4  ... in subtype_dealloc () from /lib64/libpython3.6m.so.1.0
        // #5  ... in insertdict () from /lib64/libpython3.6m.so.1.0
        // #6  ... in _PyModule_ClearDict () from /lib64/libpython3.6m.so.1.0
        // #7  ... in PyImport_Cleanup () from /lib64/libpython3.6m.so.1.0
        // #8  ... in Py_FinalizeEx () from /lib64/libpython3.6m.so.1.0
        // #9  ... in commands::python::py_finalize ()
        // ....
        // #15 ... in hgmain::main () eden/scm/exec/hgmain/src/main.rs:81
        //
        // (The SIGABRT was triggered by running test-fastlog.t)
        //
        // In case `Py_Finalize` was done by something else (ex. at the end of `Py_Main`),
        // do not call `Py_Finalize` again.
        let enabled = ENABLED.fetch_and(false, Ordering::AcqRel);
        if enabled && ffi::Py_IsInitialized() != 0 {
            info_span!("Finalize Python").in_scope(|| {
                ffi::PyGILState_Ensure();
                ffi::Py_Finalize();
            });
        }
    }
}
