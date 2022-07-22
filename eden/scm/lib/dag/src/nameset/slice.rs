/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::collections::HashSet;
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Release;
use std::sync::Arc;

use futures::lock::Mutex;
use futures::StreamExt;
use indexmap::IndexSet;
use tracing::debug;
use tracing::instrument;
use tracing::trace;
use tracing::Level;

use super::hints::Flags;
use super::AsyncNameSetQuery;
use super::BoxVertexStream;
use super::Hints;
use super::NameSet;
use crate::fmt::write_debug;
use crate::Result;
use crate::VertexName;

/// Slice of a set.
#[derive(Clone)]
pub struct SliceSet {
    inner: NameSet,
    hints: Hints,
    skip_count: u64,
    take_count: Option<u64>,

    // Skipped vertexes. Updated during iteration.
    skip_cache: Arc<Mutex<HashSet<VertexName>>>,
    // Taken vertexes. Updated during iteration.
    take_cache: Arc<Mutex<IndexSet<VertexName>>>,
    // If take_cache is complete.
    take_cache_complete: Arc<AtomicBool>,
}

impl SliceSet {
    pub fn new(set: NameSet, skip_count: u64, take_count: Option<u64>) -> Self {
        let hints = set.hints().clone();
        hints.update_flags_with(|mut f| {
            // Only keep compatible flags.
            f &= Flags::ID_DESC
                | Flags::ID_ASC
                | Flags::TOPO_DESC
                | Flags::HAS_MIN_ID
                | Flags::HAS_MAX_ID
                | Flags::EMPTY;
            // Add EMPTY hints if take_count is 0.
            if take_count == Some(0) {
                f |= Flags::EMPTY;
            }
            f
        });
        Self {
            inner: set,
            hints,
            skip_count,
            take_count,
            skip_cache: Default::default(),
            take_cache: Default::default(),
            take_cache_complete: Default::default(),
        }
    }

    fn is_take_cache_complete(&self) -> bool {
        self.take_cache_complete.load(Acquire)
    }

    async fn is_skip_cache_complete(&self) -> bool {
        self.skip_cache.lock().await.len() as u64 == self.skip_count
    }

    #[instrument(level=Level::DEBUG)]
    async fn populate_take_cache(&self) -> Result<()> {
        // See Iter::next. If take_count is not set, the "take" can be unbounded,
        // and take_cache won't be populated.
        assert!(self.take_count.is_some());

        // Use iter() to populate take_cache.
        let mut iter = self.iter().await?;
        while let Some(_) = iter.next().await {}
        assert!(self.is_take_cache_complete());

        Ok(())
    }
}

struct Iter {
    inner_iter: BoxVertexStream,
    set: SliceSet,
    index: u64,
}

const SKIP_CACHE_SIZE_THRESHOLD: u64 = 1000;

impl Iter {
    async fn next(&mut self) -> Option<Result<VertexName>> {
        if self.set.is_take_cache_complete() {
            // Fast path - no need to use inner_iter.
            let index = self.index.max(self.set.skip_count);
            let take_index = index - self.set.skip_count;
            let result = {
                let cache = self.set.take_cache.lock().await;
                cache.get_index(take_index as _).cloned()
            };
            trace!("next(index={}) = {:?} (fast path)", index, &result);
            self.index = index + 1;
            return Ok(result).transpose();
        }

        loop {
            // Slow path - use inner_iter.
            let index = self.index;
            trace!("next(index={})", index);
            let next: Option<VertexName> = match self.inner_iter.next().await {
                Some(Err(e)) => {
                    self.index = u64::MAX;
                    return Some(Err(e));
                }
                Some(Ok(v)) => Some(v),
                None => None,
            };
            self.index += 1;

            // Skip?
            if index < self.set.skip_count {
                if index < SKIP_CACHE_SIZE_THRESHOLD {
                    // Update skip_cache.
                    if let Some(v) = next.as_ref() {
                        let mut cache = self.set.skip_cache.lock().await;
                        cache.insert(v.clone());
                    }
                }
                continue;
            }

            // Take?
            let take_index = index - self.set.skip_count;
            let should_take: bool = match self.set.take_count {
                Some(count) => {
                    if take_index < count {
                        // Update take_cache.
                        let mut cache = self.set.take_cache.lock().await;
                        if take_index == cache.len() as u64 {
                            if let Some(v) = next.as_ref() {
                                cache.insert(v.clone());
                            } else {
                                // No more item in the original set.
                                self.set.take_cache_complete.store(true, Release);
                            }
                        }
                        true
                    } else {
                        self.set.take_cache_complete.store(true, Release);
                        false
                    }
                }
                None => {
                    // Do not update take_cache, since the inner
                    // set can be quite large.
                    true
                }
            };
            if should_take {
                return next.map(Ok);
            } else {
                return None;
            }
        }
    }

    fn into_stream(self) -> BoxVertexStream {
        Box::pin(futures::stream::unfold(self, |mut state| async move {
            let result = state.next().await;
            result.map(|r| (r, state))
        }))
    }
}

struct TakeCacheRevIter {
    take_cache: Arc<Mutex<IndexSet<VertexName>>>,
    index: usize,
}

impl TakeCacheRevIter {
    async fn next(&mut self) -> Option<Result<VertexName>> {
        let index = self.index;
        self.index += 1;
        let cache = self.take_cache.lock().await;
        if index >= cache.len() {
            None
        } else {
            let index = cache.len() - index - 1;
            cache.get_index(index).cloned().map(Ok)
        }
    }

    fn into_stream(self) -> BoxVertexStream {
        Box::pin(futures::stream::unfold(self, |mut state| async move {
            let result = state.next().await;
            result.map(|r| (r, state))
        }))
    }
}

#[async_trait::async_trait]
impl AsyncNameSetQuery for SliceSet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        let inner_iter = self.inner.iter().await?;
        let iter = Iter {
            inner_iter,
            set: self.clone(),
            index: 0,
        };
        Ok(iter.into_stream())
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        if let Some(_take) = self.take_count {
            self.populate_take_cache().await?;
            trace!("iter_rev({:0.6?}): use take_cache", self);
            // Use take_cache to answer RevIter. This is probably better
            // than using inner.iter_rev(), if take_count is small:
            //     [<----------------------------]
            //     [skip][take][...(need skip)...]
            let iter = TakeCacheRevIter {
                take_cache: self.take_cache.clone(),
                index: 0,
            };
            Ok(iter.into_stream())
        } else {
            // Unbounded "take_count". Reuse inner.rev_iter().
            //     [<-------------------]
            //     [skip][<---take------]
            trace!("iter_rev({:0.6?}): use inner.iter_rev()", self,);
            let count = self.count().await?;
            let iter = self.inner.iter_rev().await?;
            Ok(Box::pin(iter.take(count)))
        }
    }

    async fn count(&self) -> Result<usize> {
        let count = self.inner.count().await?;
        // consider skip_count
        let count = (count as u64).max(self.skip_count) - self.skip_count;
        // consider take_count
        let count = count.min(self.take_count.unwrap_or(u64::MAX));
        Ok(count as _)
    }

    async fn contains(&self, name: &VertexName) -> Result<bool> {
        if let Some(result) = self.contains_fast(name).await? {
            return Ok(result);
        }

        debug!("SliceSet::contains({:.6?}, {:?}) (slow path)", self, name);
        let mut iter = self.iter().await?;
        while let Some(item) = iter.next().await {
            if &item? == name {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn contains_fast(&self, name: &VertexName) -> Result<Option<bool>> {
        // Check take_cache.
        {
            let take_cache = self.take_cache.lock().await;
            let is_take_cache_complete = self.is_take_cache_complete();
            let contains = take_cache.contains(name);
            match (contains, is_take_cache_complete) {
                (_, true) | (true, _) => return Ok(Some(contains)),
                (false, false) => {}
            }
        }

        // Check skip_cache.
        // Assumes one vertex only occurs once in a set.
        let skip_contains = self.skip_cache.lock().await.contains(name);
        if skip_contains {
            return Ok(Some(false));
        }

        // Check with the original set.
        let result = self.inner.contains_fast(name).await?;
        match (result, self.is_skip_cache_complete().await) {
            // Not in the original set. Slice is a subset. Result: false.
            (Some(false), _) => Ok(Some(false)),
            // In the original set. Skip cache is completed _and_ checked
            // above (name was _not_ skipped). Result: true.
            (Some(true), true) => {
                // skip_cache was checked above
                debug_assert!(!self.skip_cache.lock().await.contains(name));
                Ok(Some(true))
            }
            // Unsure cases.
            (None, false) => Ok(None),
            (Some(true), false) => Ok(None),
            (None, true) => Ok(None),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }
}

impl fmt::Debug for SliceSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("<slice")?;
        write_debug(f, &self.inner)?;
        f.write_str(" [")?;
        if self.skip_count > 0 {
            write!(f, "{}", self.skip_count)?;
        }
        f.write_str("..")?;
        if let Some(n) = self.take_count {
            write!(f, "{}", self.skip_count + n)?;
        }
        f.write_str("]>")
    }
}

#[cfg(test)]
#[allow(clippy::redundant_clone)]
mod tests {
    use nonblocking::non_blocking_result as r;

    use super::super::tests::*;
    use super::*;

    #[test]
    fn test_basic() -> Result<()> {
        let orig = NameSet::from("a b c d e f g h i");
        let count = r(orig.count())?;

        let set = SliceSet::new(orig.clone(), 0, None);
        assert_eq!(r(set.count())?, count);
        check_invariants(&set)?;

        let set = SliceSet::new(orig.clone(), 0, Some(0));
        assert_eq!(r(set.count())?, 0);
        check_invariants(&set)?;

        let set = SliceSet::new(orig.clone(), 4, None);
        assert_eq!(r(set.count())?, count - 4);
        check_invariants(&set)?;

        let set = SliceSet::new(orig.clone(), 4, Some(0));
        assert_eq!(r(set.count())?, 0);
        check_invariants(&set)?;

        let set = SliceSet::new(orig.clone(), 0, Some(4));
        assert_eq!(r(set.count())?, 4);
        check_invariants(&set)?;

        let set = SliceSet::new(orig.clone(), 4, Some(4));
        assert_eq!(r(set.count())?, 4);
        check_invariants(&set)?;

        let set = SliceSet::new(orig.clone(), 7, Some(4));
        assert_eq!(r(set.count())?, 2);
        check_invariants(&set)?;

        let set = SliceSet::new(orig.clone(), 20, Some(4));
        assert_eq!(r(set.count())?, 0);
        check_invariants(&set)?;

        let set = SliceSet::new(orig.clone(), 20, Some(0));
        assert_eq!(r(set.count())?, 0);
        check_invariants(&set)?;

        Ok(())
    }

    #[test]
    fn test_debug() {
        let orig = NameSet::from("a b c d e f g h i");
        let set = SliceSet::new(orig.clone(), 0, None);
        assert_eq!(
            format!("{:?}", set),
            "<slice <static [a, b, c] + 6 more> [..]>"
        );
        let set = SliceSet::new(orig.clone(), 4, None);
        assert_eq!(
            format!("{:?}", set),
            "<slice <static [a, b, c] + 6 more> [4..]>"
        );
        let set = SliceSet::new(orig.clone(), 4, Some(4));
        assert_eq!(
            format!("{:?}", set),
            "<slice <static [a, b, c] + 6 more> [4..8]>"
        );
        let set = SliceSet::new(orig.clone(), 0, Some(4));
        assert_eq!(
            format!("{:?}", set),
            "<slice <static [a, b, c] + 6 more> [..4]>"
        );
    }

    quickcheck::quickcheck! {
        fn test_static_quickcheck(skip_and_take: u8) -> bool {
            let skip = (skip_and_take & 0xf) as u64;
            let take = (skip_and_take >> 4) as u64;
            let take = if take > 12 {
                None
            } else {
                Some(take as u64)
            };
            let orig = NameSet::from("a c b d e f g i h j");
            let set = SliceSet::new(orig, skip, take);
            check_invariants(&set).unwrap();
            true
        }
    }
}
