/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

//! Provides dynamic versions of the thread local stats. Dynamic here means that the name of the
//! counter is being decided in runtime. If you use the `define_stats!` to define a dynamic stat
//! then the pattern that is used to format the key and the arguments used in that pattern are
//! statically checked.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::thread::LocalKey;

use dashmap::DashMap;
use dashmap::mapref::entry::Entry as DashEntry;
use fbinit::FacebookInit;

use crate::stat_types::BoxHistogram;
use crate::stat_types::BoxLocalCounter;
use crate::stat_types::BoxLocalHistogram;
use crate::stat_types::BoxLocalTimeseries;
use crate::stat_types::BoxSingletonCounter;
use crate::stat_types::Counter;
use crate::stat_types::Histogram;
use crate::stat_types::SingletonCounter;
use crate::stat_types::Timeseries;

/// The struct to hold key and stat generators that are later being used in runtime to create new
/// stats that are being held in a map to avoid reconstruction of the same counter.
pub struct DynamicStat<T, TStatType> {
    map: RefCell<HashMap<String, TStatType>>,
    key_generator: fn(&T) -> String,
    stat_generator: fn(&str) -> TStatType,
}

impl<T, TStatType> DynamicStat<T, TStatType> {
    pub fn new(key_generator: fn(&T) -> String, stat_generator: fn(&str) -> TStatType) -> Self {
        DynamicStat {
            map: RefCell::new(HashMap::new()),
            key_generator,
            stat_generator,
        }
    }

    fn get_or_default<F, V>(&self, args: T, cb: F) -> V
    where
        F: FnOnce(&TStatType) -> V,
    {
        let key = (self.key_generator)(&args);
        let mut map = self.map.borrow_mut();
        match map.entry(key) {
            Entry::Occupied(occ) => cb(occ.get()),
            Entry::Vacant(vac) => {
                let stat = (self.stat_generator)(vac.key());
                cb(vac.insert(stat))
            }
        }
    }
}

/// The struct to hold key and stat generators that are later being used in runtime to create new
/// stats that are being held in a map to avoid reconstruction of the same counter.
pub struct DynamicStatSync<T, TStatType> {
    map: DashMap<String, TStatType>,
    key_generator: fn(&T) -> String,
    stat_generator: fn(&str) -> TStatType,
}

impl<T, TStatType> DynamicStatSync<T, TStatType> {
    pub fn new(key_generator: fn(&T) -> String, stat_generator: fn(&str) -> TStatType) -> Self {
        Self {
            map: DashMap::new(),
            key_generator,
            stat_generator,
        }
    }

    fn get_or_default<F, V>(&self, args: T, cb: F) -> V
    where
        F: FnOnce(&TStatType) -> V,
    {
        let key = (self.key_generator)(&args);
        match self.map.entry(key) {
            DashEntry::Occupied(occ) => cb(occ.get()),
            DashEntry::Vacant(vac) => {
                let stat = (self.stat_generator)(vac.key());
                cb(&vac.insert(stat))
            }
        }
    }
}

/// Similar to the Counter trait, but accepts the args parameter for accessing dynamic counters
/// created at runtime.
pub trait DynamicCounter<'a, T> {
    /// Dynamic version of `Counter::increment_value`
    fn increment_value(&'a self, value: i64, args: T);
}

impl<'a, T> DynamicCounter<'a, T> for DynamicStat<T, BoxLocalCounter> {
    fn increment_value(&'a self, value: i64, args: T) {
        self.get_or_default(args, |s| s.increment_value(value));
    }
}

impl<T> DynamicCounter<'static, T> for LocalKey<DynamicStat<T, BoxLocalCounter>> {
    fn increment_value(&'static self, value: i64, args: T) {
        self.with(|s| s.increment_value(value, args));
    }
}

/// Similar to Timeseries trait, but accepts the args parameter for accessing dynamic timeseries
/// created in runtime.
pub trait DynamicTimeseries<'a, T> {
    /// Dynamic version of `Timeseries::add_value`
    fn add_value(&'a self, value: i64, args: T);

    /// Dynamic version of `Timeseries::add_value_aggregated`
    fn add_value_aggregated(&'a self, value: i64, nsamples: u32, args: T);
}

impl<'a, T> DynamicTimeseries<'a, T> for DynamicStat<T, BoxLocalTimeseries> {
    fn add_value(&'a self, value: i64, args: T) {
        self.get_or_default(args, |s| s.add_value(value));
    }

    fn add_value_aggregated(&'a self, value: i64, nsamples: u32, args: T) {
        self.get_or_default(args, |s| s.add_value_aggregated(value, nsamples));
    }
}

impl<T> DynamicTimeseries<'static, T> for LocalKey<DynamicStat<T, BoxLocalTimeseries>> {
    fn add_value(&'static self, value: i64, args: T) {
        self.with(|s| s.add_value(value, args));
    }

    fn add_value_aggregated(&'static self, value: i64, nsamples: u32, args: T) {
        self.with(|s| s.add_value_aggregated(value, nsamples, args));
    }
}

/// Similar to the Histogram trait, but accepts the args parameter for accessing dynamic
/// histograms created at runtime.
pub trait DynamicHistogram<'a, T> {
    /// Dynamic version of `Histogram::add_value`
    fn add_value(&'a self, value: i64, args: T);

    /// Dynamic version of `Histogram::add_repeated_value`
    fn add_repeated_value(&'a self, value: i64, nsamples: u32, args: T);

    /// Flush values for testing
    fn flush(&self) {}
}

impl<'a, T> DynamicHistogram<'a, T> for DynamicStat<T, BoxLocalHistogram> {
    fn add_value(&'a self, value: i64, args: T) {
        self.get_or_default(args, |s| s.add_value(value));
    }

    fn add_repeated_value(&'a self, value: i64, nsamples: u32, args: T) {
        self.get_or_default(args, |s| s.add_repeated_value(value, nsamples));
    }
}

impl<'a, T> DynamicHistogram<'a, T> for DynamicStatSync<T, BoxHistogram> {
    fn add_value(&'a self, value: i64, args: T) {
        self.get_or_default(args, |s| s.add_value(value));
    }

    fn add_repeated_value(&'a self, value: i64, nsamples: u32, args: T) {
        self.get_or_default(args, |s| s.add_repeated_value(value, nsamples));
    }

    fn flush(&self) {
        for item in self.map.iter() {
            item.value().flush();
        }
    }
}

impl<T> DynamicHistogram<'static, T> for LocalKey<DynamicStat<T, BoxLocalHistogram>> {
    fn add_value(&'static self, value: i64, args: T) {
        self.with(|s| s.add_value(value, args));
    }

    fn add_repeated_value(&'static self, value: i64, nsamples: u32, args: T) {
        self.with(|s| s.add_repeated_value(value, nsamples, args));
    }
}

/// Similar to the SingletonCounter trait, but accepts the args parameter for accessing dynamic
/// histograms created at runtime.
pub trait DynamicSingletonCounter<'a, T> {
    /// Dynamic version of `SingletonCounter::set_value`
    fn set_value(&'a self, fb: FacebookInit, value: i64, args: T);

    /// Dynamic version of `SingletonCounter::get_value`
    fn get_value(&'a self, fb: FacebookInit, args: T) -> Option<i64>;

    /// Dynamic version of `SingletonCounter::increment_value`
    fn increment_value(&'a self, fb: FacebookInit, value: i64, args: T);
}

impl<'a, T> DynamicSingletonCounter<'a, T> for DynamicStat<T, BoxSingletonCounter> {
    fn set_value(&'a self, fb: FacebookInit, value: i64, args: T) {
        self.get_or_default(args, |s| s.set_value(fb, value));
    }

    fn get_value(&'a self, fb: FacebookInit, args: T) -> Option<i64> {
        self.get_or_default(args, |s| s.get_value(fb))
    }

    fn increment_value(&'a self, fb: FacebookInit, value: i64, args: T) {
        self.get_or_default(args, |s| s.increment_value(fb, value))
    }
}

impl<T> DynamicSingletonCounter<'static, T> for LocalKey<DynamicStat<T, BoxSingletonCounter>> {
    fn set_value(&'static self, fb: FacebookInit, value: i64, args: T) {
        self.with(|s| s.set_value(fb, value, args))
    }

    fn get_value(&'static self, fb: FacebookInit, args: T) -> Option<i64> {
        self.with(|s| s.get_value(fb, args))
    }

    fn increment_value(&'static self, fb: FacebookInit, value: i64, args: T) {
        self.with(|s| s.increment_value(fb, value, args))
    }
}
