/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use once_cell::sync::Lazy;
use parking_lot::RwLock;

use crate::StaticBox;

/// Intern a string.
fn intern(s: &str) -> &'static str {
    let set = INTERNED_STRINGS.upgradable_read();
    if let Some(s) = set.get(s) {
        return s.static_ref();
    }
    let mut set = parking_lot::lock_api::RwLockUpgradableReadGuard::upgrade(set);
    // TODO: Use get_or_insert_owned once stabilized (https://github.com/rust-lang/rust/issues/60896)
    if !set.contains(s) {
        let inserted = set.insert(StaticBox::new(s.to_string()));
        debug_assert!(inserted);
    }
    set.get(s).unwrap().static_ref()
}

/// Syntax sugar.
pub(crate) trait Intern {
    type Target;
    fn intern(&self) -> Self::Target;
}

impl Intern for String {
    type Target = &'static str;
    fn intern(&self) -> Self::Target {
        intern(self.as_str())
    }
}

impl Intern for Option<String> {
    type Target = Option<&'static str>;
    fn intern(&self) -> Self::Target {
        self.as_ref().map(|s| intern(s.as_str()))
    }
}

impl Intern for str {
    type Target = &'static str;
    fn intern(&self) -> Self::Target {
        intern(self)
    }
}

impl Intern for Option<&str> {
    type Target = Option<&'static str>;
    fn intern(&self) -> Self::Target {
        self.map(|s| intern(s))
    }
}

/// Collection of interned strings.
pub(crate) static INTERNED_STRINGS: Lazy<RwLock<HashSet<StaticBox<String>>>> =
    Lazy::new(|| Default::default());
