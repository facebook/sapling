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
use std::hash::Hash;
use std::sync::Arc;
use std::sync::RwLock;

use bitflags::bitflags;

/// Minimal abstraction for tree-like.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Item<T> {
    Tree(T, Vec<Item<T>>),
    Item(T),
    Placeholder(Placeholder),
}

/// Placeholder for capturing. Currently supports single item (`__`, like `?` in
/// glob) and mult-item (`___`, like `*` in glob), with `g` to indicate matching
/// trees (groups).
/// Might be extended (like, adding fields of custom functions) to support more
/// complex matches (ex. look ahead, balanced brackets, limited tokens, etc).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Placeholder {
    name: String,
}

impl Placeholder {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    // true: match 0 or many items; false: match 1 item
    pub fn matches_multiple(&self) -> bool {
        self.name.starts_with("___")
    }

    // true: match Item::Tree; false: does not match Item::Tree
    pub fn matches_tree(&self) -> bool {
        self.name.contains('g')
    }
}

/// Similar to regex match. A match can have multiple captures.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct Match<T> {
    /// Length of the match. We don't track the "start" since it's handled by
    /// `replace_in_place` locally.
    len: usize,
    /// Start of the match. `items[start .. start + len]` matches `pat`.
    start: usize,
    /// Placeholder -> matched items.
    pub captures: Captures<T>,
}
type Captures<T> = HashMap<String, Vec<Item<T>>>;

/// Replace matches. Similar to Python `re.sub` but is tree aware.
pub fn replace_all<T: fmt::Debug + Clone + PartialEq>(
    mut items: Vec<Item<T>>,
    pat: &[Item<T>],
    replace: impl Replace<T>,
) -> Vec<Item<T>> {
    replace_in_place(&mut items, pat, &replace);
    items
}

/// Find matches. Similar to Python `re.findall` but is tree aware.
pub fn find_all<T: fmt::Debug + Clone + PartialEq>(
    items: &[Item<T>],
    pat: &[Item<T>],
) -> Vec<Match<T>> {
    TreeMatchState::default().find_all(items, pat)
}

/// Takes a single match and output its replacement.
pub trait Replace<T> {
    fn expand(&self, m: &Match<T>) -> Vec<Item<T>>;
}

impl<T: Clone> Replace<T> for &[Item<T>] {
    fn expand(&self, m: &Match<T>) -> Vec<Item<T>> {
        expand_replace(self, &m.captures)
    }
}

impl<T: Clone> Replace<T> for &Vec<Item<T>> {
    fn expand(&self, m: &Match<T>) -> Vec<Item<T>> {
        expand_replace(self, &m.captures)
    }
}

impl<T: Clone> Replace<T> for Vec<Item<T>> {
    fn expand(&self, m: &Match<T>) -> Vec<Item<T>> {
        expand_replace(self, &m.captures)
    }
}

impl<T, F> Replace<T> for F
where
    F: Fn(&'_ Match<T>) -> Vec<Item<T>>,
{
    fn expand(&self, m: &Match<T>) -> Vec<Item<T>> {
        (self)(m)
    }
}

/// Replace matches in place.
fn replace_in_place<T: fmt::Debug + Clone + PartialEq>(
    items: &mut Vec<Item<T>>,
    pat: &[Item<T>],
    replace: &dyn Replace<T>,
) -> bool {
    let mut changed = false;
    let mut i = 0;
    while i < items.len() {
        if let Some(matched) = match_items(&items[i..], pat, true) {
            // Replace in place.
            let replaced = replace.expand(&matched);
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
            Item::Placeholder(p) => {
                if let Some(items) = captures.get(p.name()) {
                    result.extend_from_slice(items);
                }
            }
            _ => result.push(item.clone()),
        }
    }
    result
}

/// Match state for trees.
#[derive(Clone)]
struct TreeMatchState<'a, T> {
    /// (pat, items) => SeqMatchState.
    /// Only caches `allow_remaining = false` cases.
    cache: Arc<RwLock<HashMap<TreeMatchCacheKey, Arc<SeqMatchState<'a, T>>>>>,
}

/// Turn `&[Item<T>]` Eq / Hash from O(N) to O(1) based on address.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
struct TreeMatchCacheKey {
    pat: (usize, usize),
    items: (usize, usize),
    opts: TreeMatchMode,
}

/// Match state focused on one depth level.
struct SeqMatchState<'a, T> {
    parent: TreeMatchState<'a, T>,
    cache: Vec<SeqMatched>,
    pat: &'a [Item<T>],
    items: &'a [Item<T>],
    /// Matched length. None: not matched.
    match_end: Option<usize>,
    /// Only set for TreeMatchMode::Search. All matched ends.
    match_ends: Vec<usize>,
}

/// Options for `TreeMatchState::match`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
enum TreeMatchMode {
    /// `pat` must match `items`, consuming the entire sequence.
    MatchFull,
    /// `pat` can match `items[..subset]`, not the entire `items`.
    MatchBegin,
    /// Perform a search to find all matches. Start / end / depth do not
    /// have to match.
    #[allow(dead_code)]
    Search,
}

bitflags! {
    /// Match state used by SeqMatchState.
    /// How an item matches a pattern. Note: there could be multiple different ways to match.
    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    struct SeqMatched: u8 {
        /// Match a single item, not a placeholder.
        const MATCH_ITEM = 1;
        /// Match a single tree, not recursive, not a placeholder.
        const MATCH_TREE = 2;
        /// Match a single item (`?` in glob) placeholder.
        const MATCH_PLACEHOLDER_SINGLE = 4;
        /// Match a multi-item (wildcard, `*` in glob) placeholder.
        const MATCH_PLACEHOLDER_MULTI = 8;
        /// Match a multi-item placeholder by extending its matched items.
        const MATCH_PLACEHOLDER_MULTI_EXTEND = 16;
        /// Hard-coded match at boundary.
        const MATCH_INIT = 32;
        /// Not yet calculated.
        const UNKNOWN = 128;
    }
}

// T does not have to implement "Default".
impl<'a, T> Default for TreeMatchState<'a, T> {
    fn default() -> Self {
        Self {
            cache: Default::default(),
        }
    }
}

impl TreeMatchCacheKey {
    fn new<T>(pat: &[T], items: &[T], opts: TreeMatchMode) -> Self {
        Self {
            pat: (pat.as_ptr() as usize, pat.len()),
            items: (items.as_ptr() as usize, items.len()),
            opts,
        }
    }
}

impl SeqMatched {
    fn has_match(self) -> bool {
        !self.is_empty()
    }
}

impl<'a, T: PartialEq + Clone + fmt::Debug> SeqMatchState<'a, T> {
    /// Whether pat[..pat_end] matches items[..item_end].
    /// Dynamic programming. O(len(pat) * len(items)) worst case for this single level.
    /// Deeper-level matches require more time complexity.
    /// For `TreeMatchMode::Search`, do not check deeper levels.
    fn matched(&mut self, pat_end: usize, item_end: usize, opts: TreeMatchMode) -> SeqMatched {
        let cached = *self.get_cache_mut(pat_end, item_end);
        if cached != SeqMatched::UNKNOWN {
            return cached;
        }
        let result = match (pat_end, item_end) {
            (0, 0) => SeqMatched::MATCH_INIT,
            (0, _) if matches!(opts, TreeMatchMode::Search) => {
                // search mode: the start does not have to match.
                SeqMatched::MATCH_INIT
            }
            (1, 0) if matches!(&self.pat[pat_end - 1], Item::Placeholder(p) if p.matches_multiple()) => {
                SeqMatched::MATCH_PLACEHOLDER_MULTI
            }
            (_, 0) | (0, _) => SeqMatched::empty(),
            _ => {
                let mut result = SeqMatched::empty();
                match &self.pat[pat_end - 1] {
                    Item::Tree(t1, pat_children) => {
                        if let Item::Tree(t2, item_children) = &self.items[item_end - 1] {
                            // The order of the conditions start from the easiest to the (maybe) hardest.
                            if t1 == t2 /* not recursive */ && self.matched(pat_end - 1, item_end - 1, opts).has_match() && self.parent.matched(pat_children, item_children, TreeMatchMode::MatchFull).has_match()
                            {
                                result |= SeqMatched::MATCH_TREE;
                            }
                        }
                    }
                    Item::Item(t1) => {
                        if matches!(&self.items[item_end - 1], Item::Item(t2) if t1 == t2)
                            && self.matched(pat_end - 1, item_end - 1, opts).has_match()
                        {
                            result |= SeqMatched::MATCH_ITEM;
                        }
                    }
                    Item::Placeholder(p) => {
                        let match_tree = p.matches_tree();
                        if p.matches_multiple() {
                            // item: . . . .
                            //            /
                            // pat:  . . . p (new match against empty slice)
                            if self.matched(pat_end - 1, item_end, opts).has_match() {
                                result |= SeqMatched::MATCH_PLACEHOLDER_MULTI;
                            }
                            // item: . . . .
                            //            \|
                            // pat:  . . . p (extend match)
                            let m = self.matched(pat_end, item_end - 1, opts);
                            if m.intersects(
                                SeqMatched::MATCH_PLACEHOLDER_MULTI
                                    | SeqMatched::MATCH_PLACEHOLDER_MULTI_EXTEND,
                            ) {
                                if match_tree
                                    || !matches!(&self.items[item_end - 1], Item::Tree(..))
                                {
                                    result |= SeqMatched::MATCH_PLACEHOLDER_MULTI_EXTEND;
                                }
                            }
                        } else if (match_tree
                            || !matches!(&self.items[item_end - 1], Item::Tree(..)))
                            && self.matched(pat_end - 1, item_end - 1, opts).has_match()
                        {
                            result |= SeqMatched::MATCH_PLACEHOLDER_SINGLE;
                        }
                    }
                };
                result
            }
        };
        assert!(!result.contains(SeqMatched::UNKNOWN));
        *self.get_cache_mut(pat_end, item_end) = result;
        result
    }

    /// Backtrack the match and fill `captures`.
    fn fill_match(&self, r#match: &mut Match<T>) {
        self.fill_match_with_match_end(r#match, None, true);
    }

    /// Backtrace all matches. Used together with `TreeMatchMode::Search`.
    ///
    /// Matches are reported from start to end picking the longest.
    /// Overlapping matches are skipped.
    ///
    /// DOES NOT fill matches in nested trees.
    /// `SeqMatchState` only calculates matches at the current layer.
    fn fill_matches(&self, matches: &mut Vec<Match<T>>) {
        for &end in &self.match_ends {
            let mut m = Match {
                captures: Default::default(),
                len: 0,
                start: 0,
            };
            // Just figures out the matching start position so we can check overalp
            // and maybe replace the last match.
            // There are probably smarter ways to handle this...
            self.fill_match_with_match_end(&mut m, Some(end), false);
            assert_eq!(m.start + m.len, end);
            if let Some(last) = matches.last() {
                if last.start >= m.start {
                    assert!(last.start + last.len < m.start + m.len);
                    // Current match is better than last. Replace last.
                    matches.pop();
                } else if last.start + last.len > m.start {
                    // Current match overlaps with last. Skip current.
                    continue;
                }
            }
            // Update the "captures".
            self.fill_match_with_match_end(&mut m, Some(end), true);
            matches.push(m);
        }
    }

    // If fill_captures is false, still report match start.
    fn fill_match_with_match_end(
        &self,
        r#match: &mut Match<T>,
        match_end: Option<usize>,
        fill_captures: bool,
    ) {
        let mut pat_len = self.pat.len();
        let mut multi_len = 0;
        let match_end = match_end.unwrap_or_else(|| self.match_end.unwrap());
        let mut item_len = match_end;
        loop {
            let mut item_dec = 1;
            let matched = self.get_cache(pat_len, item_len);
            if matched.contains(SeqMatched::MATCH_ITEM) {
                pat_len -= 1;
            } else if matched.contains(SeqMatched::MATCH_TREE) {
                if let (Item::Tree(_, pat_children), Item::Tree(_, item_children)) =
                    (&self.pat[pat_len - 1], &self.items[item_len - 1])
                {
                    if fill_captures {
                        self.parent
                            .matched(pat_children, item_children, TreeMatchMode::MatchFull)
                            .fill_match_with_match_end(r#match, None, fill_captures);
                    }
                    pat_len -= 1;
                } else {
                    unreachable!("bug: MATCH_TREE does not actually match trees");
                }
            } else if matched.contains(SeqMatched::MATCH_PLACEHOLDER_MULTI_EXTEND) {
                multi_len += 1;
            } else if matched.intersects(
                SeqMatched::MATCH_PLACEHOLDER_MULTI | SeqMatched::MATCH_PLACEHOLDER_SINGLE,
            ) {
                let (start, len) = if matched.intersects(SeqMatched::MATCH_PLACEHOLDER_SINGLE) {
                    (item_len - 1, 1)
                } else {
                    item_dec = 0;
                    (item_len, multi_len)
                };
                if let Item::Placeholder(p) = &self.pat[pat_len - 1] {
                    if fill_captures {
                        r#match.captures.insert(
                            p.name().to_string(),
                            self.items[start..start + len].to_vec(),
                        );
                    }
                } else {
                    unreachable!("bug: MATCH_PLACEHOLDER does not actually match a placeholder");
                }
                pat_len -= 1;
                multi_len = 0;
            }
            if pat_len == 0 && item_len > 0 {
                item_len -= item_dec;
                break;
            }
            if item_len == 0 {
                break;
            } else {
                item_len -= item_dec;
            }
        }
        r#match.start = item_len;
        r#match.len = match_end - r#match.start;
    }

    /// Cached match result for calculate(pat_end, item_end).
    fn get_cache_mut(&mut self, pat_end: usize, item_end: usize) -> &mut SeqMatched {
        debug_assert!(pat_end <= self.pat.len() && item_end <= self.items.len());
        &mut self.cache[(item_end) * (self.pat.len() + 1) + pat_end]
    }

    fn get_cache(&self, pat_end: usize, item_end: usize) -> SeqMatched {
        debug_assert!(pat_end <= self.pat.len() && item_end <= self.items.len());
        self.cache[(item_end) * (self.pat.len() + 1) + pat_end]
    }

    fn has_match(&self) -> bool {
        self.match_end.is_some()
    }
}

impl<'a, T: PartialEq + Clone + fmt::Debug> TreeMatchState<'a, T> {
    /// Match items. `pat` must match `items` from start to end.
    fn matched(
        &self,
        pat: &'a [Item<T>],
        items: &'a [Item<T>],
        opts: TreeMatchMode,
    ) -> Arc<SeqMatchState<'a, T>> {
        let key = TreeMatchCacheKey::new(pat, items, opts);
        if let Some(cached) = self.cache.read().unwrap().get(&key) {
            return cached.clone();
        }

        let parent = self.clone();
        let cache = vec![SeqMatched::UNKNOWN; (items.len() + 1) * (pat.len() + 1)];
        let mut seq = SeqMatchState {
            parent,
            cache,
            pat,
            items,
            match_end: None,
            match_ends: Default::default(),
        };
        match opts {
            TreeMatchMode::MatchFull => {
                if !seq.matched(pat.len(), items.len(), opts).is_empty() {
                    seq.match_end = Some(items.len());
                }
            }
            TreeMatchMode::MatchBegin | TreeMatchMode::Search => {
                // Figure out the longest match.
                let is_search = opts == TreeMatchMode::Search;
                for len in 1..=items.len() {
                    if !seq.matched(pat.len(), len, opts).is_empty() {
                        seq.match_end = Some(len);
                        if is_search {
                            seq.match_ends.push(len);
                        }
                    }
                }
            }
        }
        self.cache
            .write()
            .unwrap()
            .entry(key)
            .or_insert(Arc::new(seq))
            .clone()
    }

    fn find_all(&self, items: &'a [Item<T>], pat: &'a [Item<T>]) -> Vec<Match<T>> {
        let matched = self.matched(pat, items, TreeMatchMode::Search);
        let mut result = Vec::new();
        matched.fill_matches(&mut result);
        let current_depth_matches_len = result.len();
        // fill_matches() only reports matches in depth 0. Need to scan children recursively.
        for (i, item) in items.iter().enumerate() {
            if let Item::Tree(.., children) = item {
                if is_covered(i, &result[..current_depth_matches_len]) {
                    // Do not overlap.
                    continue;
                }
                result.extend(self.find_all(children, pat));
            }
        }
        result
    }
}

fn is_covered<T>(index: usize, sorted_matches: &[Match<T>]) -> bool {
    let m = match sorted_matches.binary_search_by_key(&index, |m| m.start) {
        Ok(idx) => sorted_matches.get(idx),
        Err(idx) => sorted_matches.get(idx.saturating_sub(1)),
    };
    if let Some(m) = m {
        if m.start <= index && m.start + m.len > index {
            return true;
        }
    }
    false
}

/// Match patterns in items from the start. Similar to Python's `re.match`.
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
    let opts = if allow_remaining {
        TreeMatchMode::MatchBegin
    } else {
        TreeMatchMode::MatchFull
    };
    let tree_match = TreeMatchState {
        cache: Default::default(),
    };
    let matched = tree_match.matched(pat, items, opts);
    if matched.has_match() {
        let mut r#match = Match {
            captures: Default::default(),
            len: 0,
            start: 0,
        };
        matched.fill_match(&mut r#match);
        Some(r#match)
    } else {
        None
    }
}
