/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

/// Union 2 map changes. Prefer "this" on conflict.
///
/// Practically, set `this` to the metalog that has a more recent timestamp to
/// get "more recent" results.
fn union_maps<K, T>(
    mut this: BTreeMap<K, T>,
    other: &BTreeMap<K, T>,
    ancestor: &BTreeMap<K, T>,
) -> BTreeMap<K, T>
where
    T: PartialEq + Clone,
    K: Clone + Ord,
{
    for (key, other_value) in other {
        let this_value = this.get(key);
        let ancestor_value = ancestor.get(key);
        // Prefer "this" if changed by "this".
        if this_value != ancestor_value {
            continue;
        }
        // Changed or inserted by "other".
        if ancestor_value != Some(other_value) {
            this.insert(key.clone(), other_value.clone());
        }
    }
    for (key, ancestor_value) in ancestor {
        // Deleted by "other"
        if !other.contains_key(key) {
            if let Some(this_value) = this.get(key) {
                if this_value == ancestor_value {
                    this.remove(key);
                }
            }
        }
    }
    this
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_union_maps() {
        let to_map = |s: &str| -> BTreeMap<String, String> {
            s.split_whitespace()
                .map(|s| {
                    let (k, v) = s.split_once(':').unwrap();
                    (k.to_owned(), v.to_owned())
                })
                .collect()
        };
        let t = |a: &str, b: &str, c: &str| -> String {
            let m = union_maps(to_map(a), &to_map(b), &to_map(c));
            m.iter()
                .map(|(k, v)| format!("{k}:{v}"))
                .collect::<Vec<_>>()
                .join(" ")
        };

        assert_eq!(t("a:1", "b:2", ""), "a:1 b:2", "both add");
        assert_eq!(t("a:1 b:2", "b:2 c:3", "a:1 b:2 c:3"), "b:2", "both delete");
        assert_eq!(
            t("a:2 b:2 c:3", "a:1 b:2 c:2", "a:1 b:2 c:3"),
            "a:2 b:2 c:2",
            "both edit"
        );
        assert_eq!(
            t("a:5 b:7", "b:8 c:6", "a:1 b:2 c:3"),
            "a:5 b:7",
            "pick lhs on conflict"
        );
    }
}
