/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::ffi::OsString;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use parking_lot::MutexGuard;

impl<'a> EnvLock<'a> {
    pub fn new(guard: MutexGuard<'a, ()>) -> Self {
        Self {
            vars: HashMap::new(),
            _guard: guard,
        }
    }

    pub fn set(&mut self, name: impl ToString, val: Option<&str>) {
        let var = self
            .vars
            .entry(name.to_string())
            .or_insert_with(|| ScopedEnvVar::new(name));

        var.set(val);
    }
}

pub struct ScopedEnvVar {
    name: String,
    old: Option<OsString>,
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        self.set(self.old.as_deref())
    }
}

impl ScopedEnvVar {
    pub fn new(name: impl ToString) -> Self {
        let name = name.to_string();
        let old = env::var_os(&name);
        Self { name, old }
    }

    pub fn set(&self, val: Option<impl AsRef<OsStr>>) {
        match val {
            None => env::remove_var(&self.name),
            Some(val) => env::set_var(&self.name, val),
        }
    }
}

/// EnvLock allows changing env vars that are unset automatically when
/// the EnvLock goes out of scope.
pub struct EnvLock<'a> {
    // vars must be declared first to get dropped before mutex guard.
    vars: HashMap<String, ScopedEnvVar>,

    _guard: MutexGuard<'a, ()>,
}

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// Lock the environment and return an object that allows setting env
/// vars, undoing env changes when the object goes out of scope. This
/// should be used by tests that rely on particular environment
/// variable values that might be overwritten by other tests.
pub fn lock_env() -> EnvLock<'static> {
    EnvLock::new(ENV_LOCK.lock())
}
