/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Convert between foreign types and containers of foreign types.
//! - dag::Id <-> i32 (Javascript / WASM has issues with u64)
//! - dag::Vertex <-> String
//! - Containers (Vec, BTreeMap, BTreeSet, HashMap) of these types.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::hash::Hash;

use dag::Group;
use dag::Id;
use dag::Vertex;

/// Similar to Into but bypass Rust's orphan rule.
pub trait Convert<T> {
    fn convert(self) -> T;
}

impl Convert<Vertex> for String {
    fn convert(self) -> Vertex {
        Vertex::from(self.into_bytes())
    }
}

impl Convert<String> for Vertex {
    fn convert(self) -> String {
        std::str::from_utf8(self.as_ref()).unwrap().to_string()
    }
}

impl Convert<i32> for Id {
    fn convert(self) -> i32 {
        let mut v: i32 = (self.0 - self.group().min_id().0) as i32;
        if self.group() == Group::NON_MASTER {
            // Use negative number for non-master group.
            v = -v - 1;
        }
        v
    }
}

impl Convert<Id> for i32 {
    fn convert(self) -> Id {
        if self < 0 {
            Group::NON_MASTER.min_id() + (1 - self) as u64
        } else {
            Id(self as u64)
        }
    }
}

impl Convert<String> for String {
    fn convert(self) -> String {
        self
    }
}

impl<T1, T2> Convert<Vec<T2>> for Vec<T1>
where
    T1: Convert<T2>,
{
    fn convert(self) -> Vec<T2> {
        self.into_iter().map(|v| v.convert()).collect()
    }
}

impl<T1, T2> Convert<Vec<T2>> for BTreeSet<T1>
where
    T1: Convert<T2>,
{
    fn convert(self) -> Vec<T2> {
        self.into_iter().map(|v| v.convert()).collect()
    }
}

impl<T1, T2> Convert<BTreeSet<T2>> for Vec<T1>
where
    T1: Convert<T2>,
    T2: Ord,
{
    fn convert(self) -> BTreeSet<T2> {
        self.into_iter().map(|v| v.convert()).collect()
    }
}

impl<T1, T2> Convert<BTreeSet<T2>> for BTreeSet<T1>
where
    T1: Convert<T2>,
    T2: Ord,
{
    fn convert(self) -> BTreeSet<T2> {
        self.into_iter().map(|v| v.convert()).collect()
    }
}

impl<T1, T2> Convert<Option<T2>> for Option<T1>
where
    T1: Convert<T2>,
{
    fn convert(self) -> Option<T2> {
        self.map(|v| v.convert())
    }
}

impl<K1, K2, V1, V2> Convert<HashMap<K2, V2>> for HashMap<K1, V1>
where
    K1: Convert<K2>,
    V1: Convert<V2>,
    K2: Eq + Hash,
{
    fn convert(self) -> HashMap<K2, V2> {
        self.into_iter()
            .map(|(k, v)| (k.convert(), v.convert()))
            .collect()
    }
}

impl<K1, K2, V1, V2> Convert<BTreeMap<K2, V2>> for BTreeMap<K1, V1>
where
    K1: Convert<K2>,
    V1: Convert<V2>,
    K2: Eq + Ord,
{
    fn convert(self) -> BTreeMap<K2, V2> {
        self.into_iter()
            .map(|(k, v)| (k.convert(), v.convert()))
            .collect()
    }
}
