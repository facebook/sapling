/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Result;
use parking_lot::Mutex;
use parking_lot::RwLock;

use crate::Atom;
use crate::GetAtomValue;

/// Main state. Holds various types of values. Cheap to clone.
///
/// Tracks two types of values:
/// - Primitive: no dependencies, "free" variables.
/// - Derived: derived from other values.
#[derive(Default)]
pub struct Store {
    inner: Arc<RwLock<StoreInner>>,
    recording_deps: Option<Mutex<Deps>>,
}

impl Store {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update a value. Mark it as changed.
    pub fn set<V: Atom>(&self, value: impl Into<Arc<V::Value>>) {
        let key = key_of::<V>();
        let force = V::has_interior_mutability();
        let value = value.into();
        self.set_impl(key, value as ArcAny, force, &|| recalc_func::<V>());
    }

    /// Get a value. Calculate on demand.
    pub fn get<V: Atom>(&self) -> Result<Arc<V::Value>> {
        let key = key_of::<V>();
        let value = self.get_impl(key, &|| recalc_func::<V>())?;
        let value = value.downcast::<V::Value>().unwrap();
        Ok(value)
    }

    /// Similar to `self.set(RwLock::new(value))`, but mark the value as changed
    /// when the write lock guard is dropped.
    /// Returns the `Arc<RwLock>` that can be modified and trigger store refresh.
    #[must_use]
    pub fn set_rwlock<V: Atom, T>(&self, value: T) -> Arc<V::Value>
    where
        V::Value: From<crate::RwLock<T>>,
    {
        let lock = crate::RwLock::new(value);
        // safety: not calling unbalanced lock / unlock here.
        let raw = unsafe { lock.raw() };
        raw.touch_store_on_change::<V>(self);
        let value: Arc<V::Value> = Arc::new(lock.into());
        self.set::<V>(value.clone());
        value
    }
}

impl GetAtomValue for Store {
    fn get<V: Atom>(&self) -> Result<Arc<V::Value>> {
        Store::get::<V>(self)
    }
}

impl Default for StoreInner {
    fn default() -> Self {
        Self {
            values: Default::default(),
            epoch: NonZeroUsize::new(1).unwrap(),
        }
    }
}

// Right now the `key` is just a static type id.
// In the future it might be extended to support `atomFamily` use-cases.
type Key = TypeId;

impl crate::lock::WrappedRwLock {
    /// For an `RwLock`, mark `V` as changed when `RwLock::write` guard gets dropped.
    fn touch_store_on_change<V: Atom>(&self, store: &Store) {
        let key = key_of::<V>();
        self.touch_store_on_change_impl(store, key);
    }

    fn touch_store_on_change_impl(&self, store: &Store, key: Key) {
        let weak = Arc::downgrade(&store.inner);
        let on_drop = move || {
            if let Some(inner) = weak.upgrade() {
                inner.write().touch(key);
            }
        };
        let _ = self.on_unlock_exclusive.set(Box::new(on_drop));
    }
}

// The following are internal implementation details.

struct StoreInner {
    values: HashMap<Key, ValueState>,
    /// Bumps when any value is updated. Used to test if a value is up-to-date
    /// without checking all dependencies.
    epoch: NonZeroUsize,
}

struct Dep {
    key: Key,
    version: usize,
}

type Deps = Vec<Dep>;
type ArcAny = Arc<dyn Any + Send + Sync + 'static>;

/// Actual value with its surrounding states.
struct ValueState {
    /// Actual value.
    value: ArcAny,
    /// Bumps when `value` is updated. Used to test if another `Value`'s
    /// `deps` containing this `Value`is up-to-date or not.
    version: usize,
    /// Dependency `Value`s
    deps: Arc<Deps>,
    /// If == `StoreInner`'s `epoch`, the value is confirmed up-to-date.
    /// If == `0`, the value is confirmed outdated.
    /// In both cases, no need to check `deps`.
    /// This field is updated after checking `deps` to avoid duplicated checks.
    checked_epoch: AtomicUsize,
    /// Trait object version of `calculate`.
    /// This is useful to re-calculate from bottom-up (dependency first)
    recalc: RecalcFunc,
}

type RecalcFunc = Arc<dyn Fn(&Store, Option<ArcAny>) -> Result<ArcAny> + Send + Sync>;

impl Store {
    fn get_impl(&self, key: Key, get_recalc: &dyn Fn() -> RecalcFunc) -> Result<ArcAny> {
        // Attempt to reuse the existing value under the shared read lock.
        let inner = self.inner.read();
        let (deps, prev, version, recalc) = if let Some(value_state) = inner.values.get(&key) {
            let value: ArcAny = value_state.value.clone();
            if inner.is_up_to_date(value_state) {
                // Confirmed up-to-date.
                self.record_dep(key, value_state.version);
                return Ok(value);
            } else {
                // Reuse existing states for re-calculate.
                (
                    value_state.deps.clone(),
                    Some(value),
                    value_state.version,
                    value_state.recalc.clone(),
                )
            }
        } else {
            let (deps, prev, version) = Default::default();
            (deps, prev, version, get_recalc())
        };
        drop(inner);

        // A subset of the dependency graph needs re-calc.
        // This might require the exclusive write lock.
        let (value, version) = self.recalculate(key, deps, prev, version, recalc)?;
        self.record_dep(key, version);
        Ok(value)
    }

    fn set_impl(&self, key: Key, value: ArcAny, force: bool, get_recalc: &dyn Fn() -> RecalcFunc) {
        self.inner
            .write()
            .set(key, value, Default::default(), force, get_recalc);
    }

    /// Re-calculate recursively, in this order:
    /// - Re-calculate outdated dependencies.
    /// - Re-calculate "key" if dependencies are changed.
    ///
    /// Returns the up-to-date value and version.
    fn recalculate(
        &self,
        key: Key,
        deps: Arc<Deps>,
        prev: Option<ArcAny>,
        version: usize,
        recalc: RecalcFunc,
    ) -> Result<(ArcAny, usize)> {
        // Update deps, to potentially avoid a real recalc.
        let mut need_recalc = prev.is_none();
        'deps: for Dep {
            key: dep_key,
            version: needed_dep_version,
        } in deps.iter()
        {
            let inner = self.inner.read();
            let value_state = match inner.values.get(dep_key) {
                None => {
                    // This shouldn't happen. However, if it happens, just recalc from the top.
                    need_recalc = true;
                    break 'deps;
                }
                Some(v) => v,
            };
            // Check epoch. If `is_up_to_date` was called, then `checked_epoch` is up-to-date.
            let checked_epoch = value_state.checked_epoch.load(Ordering::Acquire);
            if checked_epoch == inner.epoch.get() {
                need_recalc = need_recalc || (*needed_dep_version != value_state.version);
                continue;
            }
            // `recalculate` the dependency.
            let deps = value_state.deps.clone();
            let prev = Some(value_state.value.clone());
            let recalc = value_state.recalc.clone();
            let dep_version = value_state.version;
            drop(inner);
            let (_, new_dep_version) =
                self.recalculate(*dep_key, deps, prev, dep_version, recalc)?;
            if !need_recalc && new_dep_version != *needed_dep_version {
                // Dep changed.
                need_recalc = true;
            }
        }
        // Actual re-calculate.
        if need_recalc {
            let mut store = self.with_recording_deps();
            let value = (recalc)(&store, prev)?;
            let deps = store.recorded_deps();
            let version = self
                .inner
                .write()
                .set(key, value.clone(), deps, false, &|| recalc.clone());
            Ok((value, version))
        } else {
            Ok((prev.unwrap(), version))
        }
    }

    fn record_dep(&self, key: Key, version: usize) {
        if let Some(recording_deps) = &self.recording_deps {
            let dep = Dep { key, version };
            recording_deps.lock().push(dep);
        }
    }

    fn recorded_deps(&mut self) -> Arc<Deps> {
        Arc::new(self.recording_deps.take().unwrap_or_default().into_inner())
    }

    fn with_recording_deps(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            recording_deps: Some(Mutex::new(Vec::new())),
        }
    }
}

impl StoreInner {
    /// Test if `value` is up-to-date, recursively.
    fn is_up_to_date(&self, value_state: &ValueState) -> bool {
        // Fast path.
        let checked_epoch = value_state.checked_epoch.load(Ordering::Acquire);
        if checked_epoch == self.epoch.get() {
            return true;
        } else if checked_epoch == 0 {
            return false;
        }
        // Check dependencies.
        let result = 'check: {
            for Dep { key, version } in value_state.deps.iter() {
                let dep_value = match self.values.get(key) {
                    Some(v) => v,
                    None => break 'check false,
                };
                if dep_value.version != *version {
                    break 'check false;
                }
                // Check recursively.
                if !self.is_up_to_date(dep_value) {
                    break 'check false;
                }
            }
            true
        };
        // Confirmed up-to-date. Update the fast path.
        let checked_epoch = if result { self.epoch.get() } else { 0 };
        value_state
            .checked_epoch
            .store(checked_epoch, Ordering::Release);
        result
    }

    /// Update a value. Bump `version` and `epoch` accordingly.
    /// Returns the updated `version`.
    /// If `force` is `false`, as an optimization, do not bump `version` or
    /// `epoch` if the `value` `ptr_eq`s the existing `value`.
    fn set(
        &mut self,
        key: Key,
        value: ArcAny,
        deps: Arc<Deps>,
        force: bool,
        get_recalc: &dyn Fn() -> RecalcFunc,
    ) -> usize {
        let next_epoch = self.epoch.get().wrapping_add(1).max(1);
        let version = match self.values.entry(key) {
            Entry::Occupied(e) => {
                let v = e.into_mut();
                let bump_version = force || !Arc::ptr_eq(&v.value, &value);
                if bump_version {
                    v.value = value;
                    v.version = v.version.wrapping_add(1);
                    v.checked_epoch.store(next_epoch, Ordering::Release);
                }
                v.deps = deps;
                v.version
            }
            Entry::Vacant(e) => {
                let version = 0;
                let v = ValueState {
                    value,
                    deps,
                    checked_epoch: AtomicUsize::new(next_epoch),
                    version,
                    recalc: get_recalc(),
                };
                e.insert(v);
                version
            }
        };
        self.epoch = NonZeroUsize::new(next_epoch).unwrap();
        version
    }

    /// Bump the version of a value to mark it as changed.
    /// This is only useful for values with interior mutability.
    fn touch(&mut self, key: Key) {
        if let Some(value_state) = self.values.get_mut(&key) {
            let next_epoch = self.epoch.get().wrapping_add(1).max(1);
            value_state.version = value_state.version.wrapping_add(1);
            value_state
                .checked_epoch
                .store(next_epoch, Ordering::Release);
            self.epoch = NonZeroUsize::new(next_epoch).unwrap();
        }
    }
}

fn recalc_func<T: Atom>() -> RecalcFunc {
    Arc::new(
        |store: &Store, prev_value: Option<ArcAny>| -> Result<ArcAny> {
            let prev_value = prev_value.map(|v| v.downcast::<T::Value>().unwrap());
            T::calculate(store, prev_value).map(|v| v as _)
        },
    )
}

fn key_of<T: Atom>() -> Key {
    TypeId::of::<T>()
}
