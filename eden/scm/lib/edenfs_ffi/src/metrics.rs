/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::AddAssign;

#[cfg(feature = "ods")]
use stats::prelude::*;

#[derive(Clone, Debug, Default)]
pub struct FilteredFSMetrics {
    /// Number of times we've parsed a filter file
    lookups: usize,

    /// Number of times we've failed to parse a filter file
    lookup_failures: usize,

    /// Number of times we've been passed an invalid repo path
    invalid_repos: usize,

    /// Number of Repo cache hits (i.e. first time parsing a filter for this repo)
    repo_cache_misses: usize,

    /// Number of Repo cache misses (i.e. already parsed the filter file for this repo)
    repo_cache_hits: usize,
}

impl AddAssign for FilteredFSMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.lookups += rhs.lookups;
        self.lookup_failures += rhs.lookup_failures;
        self.invalid_repos += rhs.invalid_repos;
        self.repo_cache_misses += rhs.repo_cache_misses;
        self.repo_cache_hits += rhs.repo_cache_hits;
    }
}

impl FilteredFSMetrics {
    pub(crate) fn lookup(&mut self) {
        self.lookups += 1;
    }

    pub(crate) fn failure(&mut self) {
        self.lookup_failures += 1;
    }

    pub(crate) fn invalid_repo(&mut self) {
        self.invalid_repos += 1;
    }

    pub(crate) fn repo_miss(&mut self) {
        self.repo_cache_misses += 1;
    }

    pub(crate) fn repo_hit(&mut self) {
        self.repo_cache_hits += 1;
    }

    pub(crate) fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        [
            ("lookups", self.lookups),
            ("lookup_failures", self.lookup_failures),
            ("invalid_repo", self.invalid_repos),
            ("repo_cache_misses", self.repo_cache_misses),
            ("repo_cache_hits", self.repo_cache_hits),
        ]
        .into_iter()
    }

    /// Update ODS stats.
    /// This assumes that fbinit was called higher up the stack.
    /// It is meant to be used when called from eden which uses the `revisionstore` with
    /// the `ods` feature flag.
    #[cfg(feature = "ods")]
    pub(crate) fn update_ods(&self) -> anyhow::Result<()> {
        for (metric, value) in self.metrics() {
            // SAFETY: this is called from C++ and was init'd there
            unsafe {
                let fb = fbinit::assume_init();
                STATS::ffs.increment_value(fb, value.try_into()?, (metric.to_string(),));
            }
        }
        Ok(())
    }

    #[cfg(not(feature = "ods"))]
    pub(crate) fn update_ods(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "ods")]
define_stats! {
    prefix = "edenffi";
    ffs: dynamic_singleton_counter("ffs.{}", (specific_counter: String)),
}
