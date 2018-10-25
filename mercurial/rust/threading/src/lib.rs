#[macro_use]
extern crate cpython;

use cpython::{exc, PyErr, PyObject, PyResult, PyType, PythonObject};
use std::cell::RefCell;
use std::mem::{drop, forget, transmute, transmute_copy};
use std::ptr;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, ThreadId};
use std::time::Duration;

py_module_initializer!(threading, initthreading, PyInit_threading, |py, m| {
    m.add_class::<Condition>(py)?;
    Ok(())
});

struct MutexOwner {
    ptr: *mut MutexGuard<'static, ()>,
    owner: ThreadId,
    count: usize,
}

// We do our own checking to make sure the guard is obtained
// and released from a same thread. Therefore ignore Send/Sync
// requirement.
unsafe impl Send for MutexOwner {}
unsafe impl Sync for MutexOwner {}

fn rust_thread_id() -> ThreadId {
    thread::current().id()
}

impl MutexOwner {
    fn is_none(&self) -> bool {
        self.ptr.is_null()
    }

    fn is_some(&self) -> bool {
        !self.is_none()
    }

    fn is_owned(&self) -> bool {
        self.is_some() && self.owner == rust_thread_id()
    }

    unsafe fn from_guard_count<'a>(guard: MutexGuard<'a, ()>, count: usize) -> Self {
        // Extend lifetime and box
        let boxed_extended = { Box::new(transmute(guard)) };
        let ptr = Box::into_raw(boxed_extended);
        let owner = rust_thread_id();
        MutexOwner { ptr, owner, count }
    }

    unsafe fn into_guard_count(self) -> (MutexGuard<'static, ()>, usize) {
        if !self.is_owned() {
            panic!("into_guard called from wrong thread!");
        }
        let boxed = Box::from_raw(self.ptr);
        (*boxed, self.count)
    }

    fn incref(&mut self) {
        if !self.is_owned() {
            panic!("incref called from wrong thread!");
        }
        self.count += 1;
    }

    fn decref(&mut self) -> bool {
        // Return true if is_none() becomes true.
        if !self.is_owned() {
            panic!("decref called from wrong thread!");
        }
        if self.count > 1 {
            self.count -= 1;
            false
        } else {
            assert!(self.count == 1);
            let boxed = unsafe { Box::from_raw(self.ptr) };
            let guard = *boxed;
            drop(guard);
            self.ptr = ptr::null_mut();
            self.count = 0;
            true
        }
    }

    fn null() -> Self {
        MutexOwner {
            ptr: ptr::null_mut(),
            owner: rust_thread_id(),
            count: 0,
        }
    }
}

fn arc_incref(mutex: &Arc<Mutex<()>>) {
    let cloned = Arc::clone(mutex);
    forget(cloned);
}

unsafe fn arc_decref(mutex: &Arc<Mutex<()>>) {
    let cloned: &Arc<Mutex<()>> = transmute_copy(mutex);
    drop(cloned)
}

// The Condition class can be used as RLock too.
py_class!(class Condition |py| {
    // The order of data fields matters. First declared, first dropped.
    // Condvar needs to be dropped before Mutex.
    data cond: Condvar;

    // Rust expects MutexGuard to be dropped from the same thread creating it.
    //
    // However, Python might release "Condition" from any thread. To avoid
    // issues, use pointers to prevent Rust from calling MutexGuard::drop
    // automatically.
    //
    // That means, if the Python code forgets to call "condition.release()",
    // the "MutexGuard" will be leaked and the lock is not released.
    data owner: RefCell<MutexOwner>;

    // In case MutexGuard is not dropped, the mutex is still being used and
    // should not be dropped by Rust automatically. If MutexGuard was dropped,
    // the mutex should be dropped too.
    //
    // However, rust-cpython provides little interface to tp_dealloc for
    // customized clean-up logic. Therefore, rely on "drop" to release the
    // mutex.
    //
    // Here, we keep the Arc refcount in sync with MutexOwner. If there is
    // no owner, the Arc refcount can drop to 0 and the Mutex can be released.
    // Otherwise the Mutex will be leaked.
    data mutex: Arc<Mutex<()>>;

    def __new__(_cls, lock: Option<PyObject> = None) -> PyResult<PyObject> {
        match lock {
            None => {
                let cond = Condvar::new();
                let owner = RefCell::new(MutexOwner::null());
                let mutex = Arc::new(Mutex::new(()));
                Condition::create_instance(py, cond, owner, mutex).map(|c| c.into_object())
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
        if self._is_owned(py)? {
            // Reentrant. No need to lock again.
            let mut owner = self.owner(py).borrow_mut();
            owner.incref();
        } else {
            let mutex = self.mutex(py);
            let guard = if blocking {
                // Allow other threads to run "release" or "wait". Blocking.
                py.allow_threads(|| { mutex.lock().unwrap() })
            } else {
                // Try to acquire the lock. No need to unblock other threads.
                let result = mutex.try_lock();
                match result {
                    Ok(result) => result,
                    Err(_) => return Ok(false),
                }
            };
            let owner = unsafe { MutexOwner::from_guard_count(guard, 1) };
            let old_owner = self.owner(py).replace(owner);
            arc_incref(self.mutex(py));
            assert!(old_owner.is_none());
        }
        Ok(true)
    }

    def release(&self) -> PyResult<Option<bool>> {
        if self._is_owned(py)? {
            let mut owner = self.owner(py).borrow_mut();
            if owner.decref() {
                unsafe { arc_decref(self.mutex(py)); }
            }
            Ok(None)
        } else {
            Err(PyErr::new::<exc::RuntimeError, _>(py, "cannot release un-acquired lock"))
        }
    }

    // Required by RLockTests
    def _is_owned(&self) -> PyResult<bool> {
        let owner = self.owner(py).borrow();
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

        // Temporarily swap owner with None so other threads can "acquire" when
        // "cond.wait" releases the lock. 
        let owner = self.owner(py).replace(MutexOwner::null());
        let cond = self.cond(py);

        let (new_guard, count) = py.allow_threads(|| {
            // Allow other threads to run "notify", or "acquire". Blocking.
            let (guard, count) = unsafe { owner.into_guard_count() };
            let guard = match timeout {
                None => cond.wait(guard).unwrap(),
                Some(timeout) => {
                    let duration = Duration::from_millis((timeout * 1000.0) as u64);
                    cond.wait_timeout(guard, duration).unwrap().0
                }
            };
            (guard, count)
        });

        // Restore owner
        let new_owner = unsafe { MutexOwner::from_guard_count(new_guard, count) };
        let old_owner = self.owner(py).replace(new_owner);
        assert!(old_owner.is_none());

        Ok(None)
    }

    def notify(&self, n: usize = 1) -> PyResult<Option<bool>> {
        // Python API requires the lock to be acquired when using "notify",
        // although Rust does not have this restriction.
        if self._is_owned(py)? {
            let cond = self.cond(py);
            for _ in 0..n {
                cond.notify_one();
            }
            Ok(None)
        } else {
            Err(PyErr::new::<exc::RuntimeError, _>(py, "cannot notify on un-acquired lock"))
        }
    }

    def notify_all(&self) -> PyResult<Option<bool>> {
        if self._is_owned(py)? {
            self.cond(py).notify_all();
            Ok(None)
        } else {
            Err(PyErr::new::<exc::RuntimeError, _>(py, "cannot notify on un-acquired lock"))
        }
    }

    // Other stuff

    def __repr__(&self) -> PyResult<String> {
        let owner = self.owner(py).borrow();
        let msg = if owner.is_some() {
            format!("<Condition (owned by {:?}, refcount {})>", owner.owner, owner.count)
        } else {
            format!("<Condition (not owned)>")
        };
        Ok(msg)
    }
});
