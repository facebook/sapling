/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Naive find and replace implementation on a tree-ish structure.
//!
//! Intended to be used as part of Rust proc-macro logic, but separate
//! from the `proc_macro` crate for easier testing.

use std::collections::HashMap;
use std::fmt;

/// Minimal abstraction for tree-like.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Item<T> {
    Tree(T, Vec<Item<T>>),
    Item(T),
    Placeholder(String),
}

/// Similar to regex match. A match can have multiple captures.
#[derive(Debug, Clone)]
pub struct Match<T> {
    /// Length of the match. We don't track the "start" since it's handled by
    /// `replace_in_place` locally.
    len: usize,
    /// Placeholder -> matched items.
    pub captures: Captures<T>,
}
type Captures<T> = HashMap<String, Vec<Item<T>>>;

/// Replace matches. Similar to Python `re.sub` but is tree aware.
pub fn replace_all<T: fmt::Debug + Clone + PartialEq>(
    mut items: Vec<Item<T>>,
    pat: &[Item<T>],
    replace: &[Item<T>],
) -> Vec<Item<T>> {
    replace_in_place(&mut items, pat, replace);
    items
}

/// Find matches. Similar to Python `re.findall` but is tree aware.
pub fn find_all<T: fmt::Debug + Clone + PartialEq>(
    items: &[Item<T>],
    pat: &[Item<T>],
) -> Vec<Match<T>> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if let Some(matched) = match_items(&items[i..], pat, true) {
            i += matched.len.max(1);
            result.push(matched);
        } else {
            let item = &items[i];
            if let Item::Tree(_, sub_items) = item {
                // Search recursively.
                result.extend(find_all(sub_items, pat));
            }
            i += 1;
        }
    }
    result
}

/// Replace matches in place.
fn replace_in_place<T: fmt::Debug + Clone + PartialEq>(
    items: &mut Vec<Item<T>>,
    pat: &[Item<T>],
    replace: &[Item<T>],
) -> bool {
    let mut changed = false;
    let mut i = 0;
    while i < items.len() {
        if let Some(matched) = match_items(&items[i..], pat, true) {
            // Replace in place.
            let replaced = expand_replace(replace, &matched.captures);
            let replaced_len = replaced.len();
            let new_items = {
                let mut new_items = items[..i].to_vec();
                new_items.extend(replaced);
                new_items.extend_from_slice(&items[(i + matched.len)..]);
                new_items
            };
            *items = new_items;
            i += replaced_len + 1;
            changed = true;
        } else {
            let item = &mut items[i];
            if let Item::Tree(_, ref mut sub_items) = item {
                replace_in_place(sub_items, pat, replace);
            }
            i += 1;
        }
    }
    changed
}

/// Expand `replace` with captured items.
fn expand_replace<T: Clone>(replace: &[Item<T>], captures: &Captures<T>) -> Vec<Item<T>> {
    let mut result = Vec::with_capacity(replace.len());
    for item in replace {
        match item {
            Item::Tree(delimiter, sub_items) => {
                let sub_expanded = expand_replace(sub_items, captures);
                let new_tree = Item::Tree(delimiter.clone(), sub_expanded);
                result.push(new_tree);
            }
            Item::Placeholder(ident) => {
                if let Some(items) = captures.get(ident) {
                    result.extend_from_slice(items);
                }
            }
            _ => result.push(item.clone()),
        }
    }
    result
}

/// Match two item slices from the start. Similar to Python's `re.match`.
///
/// `pat` can use placeholders to match items.
///
/// If `allow_remaining` is true, `items` can have remaining parts that won't
/// be matched while there is still a successful match.
///
/// This function recursively calls itself to match inner trees.
fn match_items<T: fmt::Debug + Clone + PartialEq>(
    items: &[Item<T>],
    pat: &[Item<T>],
    allow_remaining: bool,
) -> Option<Match<T>> {
    let mut i = 0;
    let mut j = 0;
    let mut captures: Captures<T> = HashMap::new();

    'main_loop: loop {
        match (i >= items.len(), j >= pat.len(), allow_remaining) {
            (_, true, true) | (true, true, false) => return Some(Match::new(i, captures)),
            (false, true, false) => return None,
            (false, false, _) | (true, false, _) => (),
        };

        let item_next = items.get(i);
        let pat_next = &pat[j];

        // Handle placeholder matches.
        if let Item::Placeholder(ident_str) = pat_next {
            if ident_str.starts_with("___") {
                // Multi-item match (*). We just "look ahead" for a short range.
                let pat_rest = &pat[j + 1..];
                let mut item_rest = &items[i..];
                // Do not match groups, unless the placeholder wants.
                if !is_placeholder_matching_tree(ident_str) {
                    item_rest = slice_trim_trees(item_rest);
                }
                // No way to match if "item_rest" is shorter.
                if pat_rest.len() > item_rest.len() {
                    return None;
                }
                // Limit search complexity.
                const CAP: usize = 32;
                if allow_remaining && item_rest.len() > pat_rest.len() + CAP {
                    item_rest = &item_rest[..pat_rest.len() + CAP];
                }
                // Naive O(N^2) scan, but limited to CAP.
                let mut end = item_rest.len();
                let mut start = end - pat_rest.len();
                loop {
                    if pat_rest == &item_rest[start..end] {
                        // item_rest[start..end] matches the non-placeholder part of the pattern.
                        // So items[..start] matches the placeholder.
                        captures.insert(ident_str.clone(), item_rest[..start].to_vec());
                        i += end;
                        j = pat.len();
                        continue 'main_loop;
                    }
                    if !allow_remaining || start == 0 {
                        break;
                    }
                    start -= 1;
                    end -= 1;
                }
                return None;
            } else if ident_str.starts_with("__") {
                // Single item match. But do not match a tree.
                let is_matched = match item_next {
                    Some(Item::Item(_)) => true,
                    Some(Item::Tree(..)) if is_placeholder_matching_tree(ident_str) => true,
                    _ => false,
                };
                if is_matched {
                    captures.insert(ident_str.clone(), vec![item_next.unwrap().clone()]);
                    i += 1;
                    j += 1;
                    continue;
                }
                return None;
            }
        }

        // Match subtree recursively.
        if let (Some(Item::Tree(ld, lhs)), Item::Tree(rd, rhs)) = (item_next, pat_next) {
            // NOTE: we only want "shallow" tree (ex. only the brackets) check here.
            if ld != rd {
                return None;
            }
            // Match recursive.
            let sub_result = match_items(lhs, rhs, false);
            match sub_result {
                None => return None,
                Some(matched) => {
                    captures.extend(matched.captures);
                    i += 1;
                    j += 1;
                    continue;
                }
            }
        }

        // Match item.
        if item_next == Some(pat_next) {
            i += 1;
            j += 1;
        } else {
            return None;
        }
    }
}

/// Truncate a item slice so it does not have Trees.
fn slice_trim_trees<T>(slice: &[Item<T>]) -> &[Item<T>] {
    for (i, item) in slice.iter().enumerate() {
        if matches!(item, Item::Tree(..)) {
            return &slice[..i];
        }
    }
    slice
}

/// Whether the placeholder wants to match trees.
fn is_placeholder_matching_tree(ident_str: &str) -> bool {
    ident_str.contains('g')
}

impl<T> Match<T> {
    fn new(len: usize, captures: Captures<T>) -> Self {
        Self { len, captures }
    }
}
