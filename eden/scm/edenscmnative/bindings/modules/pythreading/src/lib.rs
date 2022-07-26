/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::Cell;
use std::sync::Condvar;
use std::sync::Mutex;
use std::thread;
use std::thread::ThreadId;
use std::time::Duration;

use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "threading"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<Condition>(py)?;
    m.add_class::<bug29988wrapper>(py)?;
    m.add_class::<RGenerator>(py)?;
    Ok(m)
}

#[derive(Copy, Clone)]
struct Owner {
    thread_id: ThreadId,
    count: usize,
}

// Used to pass MutexGuard into py.allow_threads closure.  "allow_threads"
// requires "Send" to prevent the use of "py". We're not passing "py" and the
// guard is owned by Rust, and the code runs in a same thread. So it is fine.
struct ForceSend<T>(T);
unsafe impl<T> Send for ForceSend<T> {}

fn rust_thread_id() -> ThreadId {
    thread::current().id()
}

impl Owner {
    fn is_none(&self) -> bool {
        self.count == 0
    }

    fn is_some(&self) -> bool {
        !self.is_none()
    }

    fn is_owned(&self) -> bool {
        self.is_some() && self.thread_id == rust_thread_id()
    }

    fn incref(self) -> Self {
        if !self.is_owned() {
            panic!("incref called from wrong thread!");
        }
        Self {
            thread_id: self.thread_id,
            count: self.count + 1,
        }
    }

    fn decref(self) -> Self {
        if !self.is_owned() {
            panic!("decref called from wrong thread!");
        }
        assert!(self.count > 0);
        Self {
            thread_id: self.thread_id,
            count: self.count - 1,
        }
    }

    fn none() -> Self {
        Self {
            thread_id: rust_thread_id(),
            count: 0,
        }
    }

    fn current_thread() -> Self {
        Self {
            thread_id: rust_thread_id(),
            count: 1,
        }
    }
}

// The Condition class can be used as RLock too.
py_class!(class Condition |py| {
    // The order of data fields matters. First declared, first dropped.

    // Wait for "notify"
    data cond_notify: Condvar;

    // Wait for "release" (internal use)
    data cond_release: Condvar;

    // Used to protect the above Condvars
    data mutex_notify: Mutex<()>;
    data mutex_release: Mutex<()>;

    // Thread owner metadata
    data owner: Cell<Owner>;


    def __new__(_cls, lock: Option<PyObject> = None) -> PyResult<PyObject> {
        match lock {
            None => {
                Ok(Condition::create_instance(
                    py,
                    Condvar::new(),
                    Condvar::new(),
                    Mutex::new(()),
                    Mutex::new(()),
                    Cell::new(Owner::none()),
                )?.into_object())
            },
            Some(lock) => {
                // Do not support taking a customized "lock".
                // Fallback to "threading._Condition" in this case.
                let threading = py.import("threading")?;
                threading.call(py, "_Condition", (lock,), None)
            }
        }
    }

    // RLock APIs

    def acquire(&self, blocking: bool = true) -> PyResult<bool> {
        let owner = self.owner(py).get();
        if owner.is_none() {
            self.owner(py).set(Owner::current_thread());
            Ok(true)
        } else if owner.is_owned() {
            let owner = owner.incref();
            self.owner(py).set(owner);
            Ok(true)
        } else {
            if blocking {
                let mutex_release = self.mutex_release(py);
                let cond_release = self.cond_release(py);
                // Blocking. Wait for other threads to "release", or "wait"
                while self.owner(py).get().is_some() {
                    let guard = ForceSend(mutex_release.lock().unwrap());
                    py.allow_threads(|| {
                        let guard = guard; // capture ForceSend into closure
                        let _guard = cond_release.wait(guard.0).unwrap();
                        // Drop _guard to release the lock before acquiring
                        // Python GIL Otherwise this might deadlock with other
                        // threads acquiring mutex_release.
                    });
                    // At this point we don't know whether the lock is free or
                    // not. The above section does not prevent a Python thread
                    // from acquiring the lock again. But we regained GIL so
                    // check it in a loop.
                }
                let old_owner = self.owner(py).replace(Owner::current_thread());
                assert!(old_owner.is_none());
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    def release(&self) -> PyResult<Option<bool>> {
        if self._is_owned(py)? {
            let owner = self.owner(py).get().decref();
            self.owner(py).set(owner);
            if owner.is_none() {
                let cond_release = self.cond_release(py);
                let _guard = self.mutex_release(py).lock().unwrap();
                cond_release.notify_one();
            }
            Ok(None)
        } else {
            Err(PyErr::new::<exc::RuntimeError, _>(py, "cannot release un-acquired lock"))
        }
    }

    // Required by RLockTests
    def _is_owned(&self) -> PyResult<bool> {
        let owner = self.owner(py).get();
        Ok(owner.is_owned())
    }

    def __enter__(&self) -> PyResult<bool> {
        self.acquire(py, true)
    }

    def __exit__(&self, _ty: Option<PyType>, _value: PyObject, _traceback: PyObject) -> PyResult<bool> {
        self.release(py).unwrap();

        // Returning False preserves exceptions
        Ok(false)
    }

    // Condition APIs

    def wait(&self, timeout: Option<f64> = None) -> PyResult<Option<bool>> {
        if !self._is_owned(py)? {
            return Err(PyErr::new::<exc::RuntimeError, _>(py, "cannot wait on un-acquired lock"));
        }

        let cond_notify = self.cond_notify(py);
        let cond_release = self.cond_release(py);
        let mutex_notify = self.mutex_notify(py);
        let mutex_release = self.mutex_release(py);

        // Temporarily release the lock so other threads can "acquire" and
        // "notify". Simplified version of "release".
        let owner = self.owner(py).replace(Owner::none());
        {
            let _guard = mutex_release.lock().unwrap();
            cond_release.notify_one();
        }

        {
            let guard = ForceSend(mutex_notify.lock().unwrap());
            py.allow_threads(|| {
                let guard = guard; // capture ForceSend into closure
                // Allow other threads to run "notify", or "acquire". Blocking.
                let _guard = match timeout {
                    None => cond_notify.wait(guard.0).unwrap(),
                    Some(timeout) => {
                        let duration = Duration::from_millis((timeout * 1000.0) as u64);
                        cond_notify.wait_timeout(guard.0, duration).unwrap().0
                    }
                };
            });
        }

        // Need to re-acquire the lock. A simplified version of "acquire".
        while self.owner(py).get().is_some() {
            let guard = ForceSend(mutex_release.lock().unwrap());
            py.allow_threads(|| {
                let guard = guard; // capture ForceSend into closure
                let _guard = cond_release.wait(guard.0).unwrap();
            });
        }

        // Restore owner
        let old_owner = self.owner(py).replace(owner);
        assert!(old_owner.is_none());

        Ok(None)
    }

    def notify(&self, n: usize = 1) -> PyResult<Option<bool>> {
        // Python API requires the lock to be acquired when using "notify",
        // although Rust does not have this restriction.
        if self._is_owned(py)? {
            let cond_notify = self.cond_notify(py);
            // Acquire the mutex. This makes sure "condvar.wait" has released
            // it. Without this, there is a small window between "allow_threads"
            // and "condvar.wait" at which time sending "notify" will be wrong.
            let _guard = self.mutex_notify(py).lock().unwrap();
            for _ in 0..n {
                cond_notify.notify_one();
            }
            Ok(None)
        } else {
            Err(PyErr::new::<exc::RuntimeError, _>(py, "cannot notify on un-acquired lock"))
        }
    }

    def notify_all(&self) -> PyResult<Option<bool>> {
        if self._is_owned(py)? {
            let _guard = self.mutex_notify(py).lock().unwrap();
            self.cond_notify(py).notify_all();
            Ok(None)
        } else {
            Err(PyErr::new::<exc::RuntimeError, _>(py, "cannot notify on un-acquired lock"))
        }
    }

    // Other stuff

    def __repr__(&self) -> PyResult<String> {
        let owner = self.owner(py).get();
        let msg = if owner.is_some() {
            format!("<Condition (owned by {:?}, refcount {})>", owner.thread_id, owner.count)
        } else {
            format!("<Condition (not owned)>")
        };
        Ok(msg)
    }
});

// To workaround Python bug 29988 where "__exit__" can be skipped by Ctrl+C.
// To use this, wrap a [Condition] in: `b = bug29988wrapper(cond)`, and use
// nested `with`: `with b, b, b, b, b: ...` in the Python world.
py_class!(class bug29988wrapper |py| {
    data obj: Condition;
    data entered: Cell<bool>;

    def __new__(_cls, obj: Condition) -> PyResult<bug29988wrapper> {
        bug29988wrapper::create_instance(py, obj, Cell::new(false))
    }

    def __enter__(&self) -> PyResult<PyObject> {
        if !self.entered(py).get() {
            let obj = self.obj(py);
            let _result = obj.acquire(py, true)?;
            self.entered(py).replace(true);
        }
        Ok(py.None())
    }

    def __exit__(&self, _ty: Option<PyType>, _value: PyObject, _traceback: PyObject) -> PyResult<bool> {
        if self.entered(py).get() {
            let obj = self.obj(py);
            obj.release(py)?;
            self.entered(py).replace(false);
        }
        Ok(false)
    }
});

// Reentrant generator. Can be iterated multiple times by using the `iter()` method.
py_class!(class RGenerator |py| {
    // Main iterator (non-reentrant).
    data iternext: Mutex<PyObject>;

    // Items produced by iter.
    data iterlist: PyList;

    // Whether iteration was completed.
    data itercompleted: Cell<bool>;

    def __new__(_cls, gen: PyObject) -> PyResult<Self> {
        Self::init(py, gen)
    }

    /// Obtains an iterator that iterates from the beginning.
    def iter(&self, skip: usize = 0) -> PyResult<RGeneratorIter> {
        RGeneratorIter::create_instance(py, self.clone_ref(py), Cell::new(skip))
    }

    /// Iterate to the end of the original generator.
    def itertoend(&self) -> PyResult<usize> {
        if self.itercompleted(py).get() {
            Ok(0)
        } else {
            let iter = self.iter(py, self.iterlist(py).len(py))?;
            let iter = ObjectProtocol::iter(iter.as_object(), py)?;
            Ok(iter.count())
        }
    }

    def list(&self) -> PyResult<PyList> {
        Ok(self.iterlist(py).clone_ref(py))
    }

    def completed(&self) -> PyResult<bool> {
        Ok(self.itercompleted(py).get())
    }
});

impl RGenerator {
    pub(crate) fn init(py: Python, gen: PyObject) -> PyResult<Self> {
        let iter = gen.iter(py)?.into_object();
        let next = match iter.getattr(py, "__next__") {
            Err(_) => iter.getattr(py, "next")?,
            Ok(next) => next,
        };
        Self::create_instance(py, Mutex::new(next), PyList::new(py, &[]), Cell::new(false))
    }
}

py_class!(class RGeneratorIter |py| {
    data rgen: RGenerator;
    data index: Cell<usize>;

    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __next__(&self) -> PyResult<Option<PyObject>> {
        // Ensure that "__next__" is atomic by locking.
        // Cannot rely on Python GIL because iternext.call(py) might release it.
        let mutex = self.rgen(py).iternext(py);
        if let Ok(locked) = mutex.try_lock() {
            self.next_internal(py, &locked)
        } else {
            // Release Python GIL to give other threads chances to release mutex.
            let locked = py.allow_threads(|| mutex.lock().unwrap());
            self.next_internal(py, &locked)
        }
    }
});

impl RGeneratorIter {
    // The caller should use locking to ensure `iternext` is not being called
    // from another thread.
    fn next_internal(&self, py: Python, iternext: &PyObject) -> PyResult<Option<PyObject>> {
        let rgen = self.rgen(py);
        let index = self.index(py).get();
        while rgen.iterlist(py).len(py) <= index && !rgen.itercompleted(py).get() {
            match iternext.call(py, NoArgs, None) {
                Ok(item) => {
                    rgen.iterlist(py).append(py, item);
                }
                Err(err) => {
                    // Could be StopIteration.
                    rgen.itercompleted(py).set(true);
                    return Err(err);
                }
            };
        }

        let result = if rgen.iterlist(py).len(py) > index {
            Some(rgen.iterlist(py).get_item(py, index))
        } else {
            None
        };

        self.index(py).set(index + 1);
        Ok(result)
    }
}

// fbcode has a whitelist of python2 executables, not including tests here
#[cfg(test)]
#[cfg(not(all(fbcode_build, feature = "python2")))]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn test_rgenerator_iter_multi_threads() {
        let rgen = with_py(|py| {
            let gen = py.eval("(i for i in range(1000))", None, None).unwrap();
            RGenerator::init(py, gen).unwrap()
        });
        let mut rgen_list = Vec::new();
        let n = 40;
        with_py(|py| {
            for _ in 0..n {
                rgen_list.push(rgen.clone_ref(py));
            }
        });

        let threads = rgen_list
            .into_iter()
            .map(move |rgen| {
                thread::spawn(move || {
                    let iter: RGeneratorIter =
                        with_py(|py| RGenerator::iter(&rgen, py, 0).unwrap());
                    let mut count = 0;
                    while let Ok(Some(_)) = with_py(|py| iter.__next__(py)) {
                        count += 1;
                    }
                    assert_eq!(count, 1000);
                    let v: Vec<u32> =
                        with_py(|py| rgen.list(py).unwrap().into_object().extract(py).unwrap());
                    assert_eq!(v, (0..1000).collect::<Vec<u32>>());
                    assert!(with_py(|py| rgen.completed(py).unwrap()));
                })
            })
            .collect::<Vec<_>>();
        for t in threads {
            t.join().unwrap();
        }
    }

    fn with_py<R>(f: impl FnOnce(Python) -> R) -> R {
        let gil = Python::acquire_gil();
        let py = gil.python();
        f(py)
    }
}
