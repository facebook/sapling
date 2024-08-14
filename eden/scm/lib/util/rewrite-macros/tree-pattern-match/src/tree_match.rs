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

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::RwLock;

use bitflags::bitflags;

/// Minimal abstraction for tree-like.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Item<T> {
    Tree(T, Vec<Item<T>>),
    Item(T),
    Placeholder(Placeholder<T>),
}

/// Placeholder for capturing. Currently supports single item (`__`, like `?` in
/// glob) and mult-item (`___`, like `*` in glob), with `g` to indicate matching
/// trees (groups).
/// Might be extended (like, adding fields of custom functions) to support more
/// complex matches (ex. look ahead, balanced brackets, limited tokens, etc).
#[derive(Clone)]
pub struct Placeholder<T> {
    name: String,
    /// If set, specify whether to match an item.
    /// Can be useful to express `[0-9a-f]` like in glob.
    matches_item: Option<Arc<dyn (Fn(&Item<T>) -> bool) + 'static>>,
    /// If true, matches empty or multiple items, like `*`.
    /// Otherwise, matches one item.
    matches_multiple: bool,
}

impl<T> PartialEq for Placeholder<T> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl<T> Eq for Placeholder<T> {}

impl<T> fmt::Debug for Placeholder<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.name.fmt(f)
    }
}

impl<T> Placeholder<T> {
    pub fn new(name: String) -> Self {
        let matches_multiple = name.starts_with("___");
        Self {
            name,
            matches_item: None,
            matches_multiple,
        }
    }

    pub fn set_matches_item(
        &mut self,
        matches_item: impl (Fn(&Item<T>) -> bool) + 'static,
    ) -> &mut Self {
        self.matches_item = Some(Arc::new(matches_item));
        self
    }

    pub fn set_matches_multiple(&mut self, matches_multiple: bool) -> &mut Self {
        self.matches_multiple = matches_multiple;
        self
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    // true: match 0 or many items; false: match 1 item
    pub fn matches_multiple(&self) -> bool {
        self.matches_multiple
    }

    /// Test matching against a single item.
    pub fn matches_item(&self, item: &Item<T>) -> bool {
        match self.matches_item.as_ref() {
            None => true,
            Some(f) => f(item),
        }
    }
}

pub trait PlaceholderExt<T> {
    fn with_placeholder_matching_items<F: Fn(&Item<T>) -> bool + 'static>(
        self,
        name_matches_item_pairs: impl IntoIterator<Item = (&'static str, F)>,
    ) -> Self;
}

impl<T> PlaceholderExt<T> for Vec<Item<T>> {
    fn with_placeholder_matching_items<F: Fn(&Item<T>) -> bool + 'static>(
        mut self,
        name_matches_item_pairs: impl IntoIterator<Item = (&'static str, F)>,
    ) -> Self {
        fn rewrite_items<T, F: Fn(&Item<T>) -> bool + 'static>(
            items: &mut [Item<T>],
            mapping: &mut HashMap<&str, F>,
        ) {
            for item in items.iter_mut() {
                match item {
                    Item::Placeholder(p) => {
                        if let Some(f) = mapping.remove(p.name()) {
                            p.set_matches_item(f);
                        }
                    }
                    Item::Tree(_, children) => {
                        rewrite_items(children, mapping);
                    }
                    _ => {}
                }
            }
        }

        let mut mapping: HashMap<&str, F> = name_matches_item_pairs.into_iter().collect();
        rewrite_items(&mut self, &mut mapping);
        self
    }
}

/// Similar to regex match. A match can have multiple captures.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Match<T> {
    /// End of the match (exclusive).
    end: usize,
    /// Start of the match. `items[start .. start + len]` matches `pat`.
    start: usize,
    /// Placeholder -> matched items.
    pub captures: Captures<T>,
}
type Captures<T> = HashMap<String, Vec<Item<T>>>;

// T does not ened to implement `Default`.
impl<T> Default for Match<T> {
    fn default() -> Self {
        Self {
            end: 0,
            start: 0,
            captures: Default::default(),
        }
    }
}

/// Replace matches. Similar to Python `re.sub` but is tree aware.
pub fn replace_all<T: fmt::Debug + Clone + PartialEq + 'static>(
    items: &[Item<T>],
    pat: &[Item<T>],
    replace: impl Replace<T>,
) -> Vec<Item<T>> {
    TreeMatchState::default()
        .replace_all(items, pat, &replace)
        .into_owned()
}

/// Find matches. Similar to Python `re.findall` but is tree aware.
pub fn find_all<T: fmt::Debug + Clone + PartialEq>(
    items: &[Item<T>],
    pat: &[Item<T>],
) -> Vec<Match<T>> {
    TreeMatchState::default().find_all(items, pat)
}

/// Find a match align with the start of items. Similar to Python `re.match`.
pub fn matches_start<T: fmt::Debug + Clone + PartialEq>(
    items: &[Item<T>],
    pat: &[Item<T>],
) -> Option<Match<T>> {
    TreeMatchState::default().find_one(items, pat, TreeMatchMode::MatchBegin)
}

/// Find a match align with items.
pub fn matches_full<T: fmt::Debug + Clone + PartialEq>(
    items: &[Item<T>],
    pat: &[Item<T>],
) -> Option<Match<T>> {
    TreeMatchState::default().find_one(items, pat, TreeMatchMode::MatchFull)
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
    /// Only set for TreeMatchMode::Search. Non-overlapping matched ends.
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
                                if p.matches_item(&self.items[item_end - 1]) {
                                    result |= SeqMatched::MATCH_PLACEHOLDER_MULTI_EXTEND;
                                }
                            }
                        } else if p.matches_item(&self.items[item_end - 1])
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

    /// Backtrack all matches. Used together with `TreeMatchMode::Search`.
    ///
    /// Matches are reported from start to end picking the longest.
    /// Overlapping matches are skipped.
    ///
    /// DOES NOT fill matches in nested trees.
    /// `SeqMatchState` only calculates matches at the current layer.
    fn fill_matches(&self, matches: &mut Vec<Match<T>>) {
        for &end in &self.match_ends {
            let mut m = Match::default();
            self.fill_match_with_match_end(&mut m, Some(end), true);
            matches.push(m);
        }
    }

    // Find the "start" of a match. This could be O(N).
    fn backtrack_match_start(&self, end: usize) -> usize {
        let mut m = Match::default();
        self.fill_match_with_match_end(&mut m, Some(end), false);
        assert_eq!(m.end, end, "find_match_start requires 'end' to be a match");
        m.start
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
        r#match.end = match_end;
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

    /// Reset cache to UNKNOWN state for item_start..item_end.
    fn clear_cache_range(&mut self, item_start: usize, item_end: usize) {
        let pat_len = self.pat.len() + 1;
        let start = item_start * pat_len;
        let end = item_end * pat_len;
        self.cache[start..end].fill(SeqMatched::UNKNOWN);
    }

    /// Modify states so new matches cannot overlap with the previous match ending at `end`.
    /// i.e. new match.start must >= end.
    fn cut_off_matches_at(&mut self, end: usize) {
        // Move the "boundary" conditions from item_end=0 to item_end=end.
        // Check `fn matched` for the boundary conditions.
        for i in 0..self.pat.len() {
            let matched = match i {
                0 => SeqMatched::MATCH_INIT,
                1 if matches!(self.pat.first(), Some(Item::Placeholder(p)) if p.matches_multiple()) => {
                    SeqMatched::MATCH_PLACEHOLDER_MULTI
                }
                _ => SeqMatched::empty(),
            };
            *self.get_cache_mut(i, end) = matched;
        }
        // cache[pat=end, item=end] is intentionally unchanged
        // so backtracking (to fill captures) still works.
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
                let mut last_cutoff = 0;
                'next: for end in 1..=items.len() {
                    'retry: loop {
                        if !seq.matched(pat.len(), end, opts).is_empty() {
                            if is_search {
                                // Deal with overlapping.
                                // There are probably smarter ways to handle this...
                                if let Some(&last_end) = seq.match_ends.last() {
                                    let start = seq.backtrack_match_start(end);
                                    let last_start = seq.backtrack_match_start(last_end);
                                    if last_start >= start {
                                        // Current match is better than last. Replace last.
                                        seq.match_ends.pop();
                                    } else if last_end > start {
                                        // Current match overlaps with last.
                                        if last_cutoff < last_end {
                                            // Re-run the search with updated cut off state.
                                            seq.clear_cache_range(last_end + 1, end + 1);
                                            seq.cut_off_matches_at(last_end);
                                            last_cutoff = last_end;
                                            continue 'retry;
                                        } else {
                                            // Already cut off. No need to re-run the search.
                                            continue 'next;
                                        }
                                    }
                                }
                                seq.match_ends.push(end);
                            }
                            seq.match_end = Some(end);
                        }
                        break;
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

    fn find_one(
        &self,
        items: &'a [Item<T>],
        pat: &'a [Item<T>],
        mode: TreeMatchMode,
    ) -> Option<Match<T>> {
        let matched = self.matched(pat, items, mode);
        matched.match_end.map(|_| {
            let mut m = Match::default();
            matched.fill_match(&mut m);
            m
        })
    }
}

impl<'a, T: PartialEq + Clone + fmt::Debug + 'static> TreeMatchState<'a, T> {
    fn replace_all(
        &self,
        items: &'a [Item<T>],
        pat: &'a [Item<T>],
        replace: &dyn Replace<T>,
    ) -> Cow<'a, [Item<T>]> {
        let matched = self.matched(pat, items, TreeMatchMode::Search);

        // Step 1. Calculate matches on the current depth.
        let mut matches = Vec::new();
        let mut replaced = MaybeOwned::<T>(OnceLock::new());
        matched.fill_matches(&mut matches);

        // Step 2. For subtrees that are not covered by the matches, replace them first.
        // This is because the replaces are 1-item to 1-item, not shifting indexes around.
        for (i, item) in items.iter().enumerate() {
            if let Item::Tree(t, children) = item {
                if is_covered(i, &matches) {
                    // Do not overlap.
                    continue;
                }
                let new_children = self.replace_all(children, pat, replace);
                if is_owned(&new_children) {
                    replaced.maybe_init(items);
                    replaced.as_mut()[i] = Item::Tree(t.clone(), new_children.into_owned());
                }
            }
        }

        // Step 3. Replace at the current depth.
        if !matches.is_empty() {
            let mut new_items = Vec::with_capacity(items.len());
            let mut end = 0;
            for m in matches {
                new_items.extend_from_slice(replaced.slice(items, end, m.start));
                let replaced = replace.expand(&m);
                new_items.extend(replaced);
                end = m.end;
            }
            new_items.extend_from_slice(replaced.slice(items, end, items.len()));
            replaced.0 = new_items.into();
        }

        match replaced.0.into_inner() {
            None => Cow::Borrowed(items),
            Some(items) => Cow::Owned(items),
        }
    }
}

// Work with Cow. This is mainly to avoid the lifetime of a `Cow`.
struct MaybeOwned<T>(OnceLock<Vec<Item<T>>>);

impl<T: Clone + 'static> MaybeOwned<T> {
    fn maybe_init(&self, init_value: &[Item<T>]) {
        self.0.get_or_init(|| init_value.to_vec());
    }

    fn slice<'a>(&'a self, fallback: &'a [Item<T>], start: usize, end: usize) -> &'a [Item<T>] {
        match self.0.get() {
            None => &fallback[start..end],
            Some(v) => &v[start..end],
        }
    }
}

impl<T: Clone + 'static> MaybeOwned<T> {
    fn as_mut(&mut self) -> &mut Vec<Item<T>> {
        self.0.get_mut().unwrap()
    }
}

// clippy: intentionally want to test on Cow but not consume it.
#[allow(clippy::ptr_arg)]
fn is_owned<T: ToOwned + ?Sized>(value: &Cow<'_, T>) -> bool {
    matches!(value, Cow::Owned(_))
}

fn is_covered<T>(index: usize, sorted_matches: &[Match<T>]) -> bool {
    let m = match sorted_matches.binary_search_by_key(&index, |m| m.start) {
        Ok(idx) => sorted_matches.get(idx),
        Err(idx) => sorted_matches.get(idx.saturating_sub(1)),
    };
    if let Some(m) = m {
        if m.start <= index && m.end > index {
            return true;
        }
    }
    false
}
