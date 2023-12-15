/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use context::CoreContext;
use context::PerfCounters;
use derived_data_constants::*;
use futures_stats::FutureStats;
use metadata::Metadata;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::warn;
use time_ext::DurationExt;

use super::derive::DerivationOutcome;
use super::util::DiscoveryStats;
use super::DerivedDataManager;
use crate::derivable::BonsaiDerivable;
use crate::error::DerivationError;

pub(super) struct DerivedDataScuba<Derivable> {
    /// Scuba sample builder to log to the derived data table.
    scuba: MononokeScubaSampleBuilder,

    /// Description of what is being derived.
    description: Option<String>,

    phantom: PhantomData<Derivable>,
}

impl DerivedDataManager {
    pub(super) fn derived_data_scuba<Derivable>(&self) -> DerivedDataScuba<Derivable>
    where
        Derivable: BonsaiDerivable,
    {
        let mut scuba = self.inner.scuba.clone();
        scuba.add("derived_data", Derivable::NAME);
        DerivedDataScuba {
            scuba,
            description: None,
            phantom: PhantomData,
        }
    }
}

impl<Derivable: BonsaiDerivable> DerivedDataScuba<Derivable> {
    /// Description of this operation to log (derived data type and affected
    /// changesets).
    fn description(&self) -> String {
        self.description
            .clone()
            .unwrap_or_else(|| Derivable::NAME.to_string())
    }

    /// Add a single changeset id to the logger.
    pub(super) fn add_changeset_id(&mut self, csid: ChangesetId) {
        self.scuba.add("changeset", csid.to_string());
        self.description = Some(format!("{} {csid}", Derivable::NAME));
    }

    /// Add a single changeset to the logger.  Logs additional data available
    /// from the bonsai changeset.
    pub(super) fn add_changeset(&mut self, bcs: &BonsaiChangeset) {
        self.add_changeset_id(bcs.get_changeset_id());
        self.scuba
            .add("changed_files_count", bcs.file_changes_map().len());
    }

    /// Add a batch of changesets to the logger.
    pub(super) fn add_changesets(&mut self, changesets: &[BonsaiChangeset]) {
        let csids = changesets
            .iter()
            .map(|bcs| bcs.get_changeset_id().to_string())
            .collect::<Vec<_>>();
        match csids.as_slice() {
            [] => {}
            [csid] => self.description = Some(format!("{} {csid}", Derivable::NAME)),
            [first, .., last] => {
                self.description = Some(format!("{} {first}-{last}", Derivable::NAME))
            }
        };
        self.scuba.add("changesets", csids);
        let changed_files_count = changesets
            .iter()
            .map(|bcs| bcs.file_changes_map().len())
            .sum::<usize>();
        self.scuba.add("changed_files_count", changed_files_count);
    }

    /// Add metadata to the logger
    pub fn add_metadata(&mut self, metadata: &Metadata) {
        self.scuba.add_metadata(metadata);
    }

    /// Add values for the parameters controlling batched derivation to the
    /// scuba logger.
    pub(super) fn add_batch_parameters(&mut self, parallel: bool, gap_size: Option<usize>) {
        self.scuba.add("parallel", parallel);
        if let Some(gap_size) = gap_size {
            self.scuba.add("gap_size", gap_size);
        }
    }

    /// Add statistics from derivation discovery to the scuba logger.
    pub(super) fn add_discovery_stats(&mut self, discovery_stats: &DiscoveryStats) {
        discovery_stats.add_scuba_fields(&mut self.scuba);
    }

    /// Log the start of derivation to both the request and derived data scuba
    /// tables.
    pub(super) fn log_derivation_start(&mut self, ctx: &CoreContext) {
        ctx.scuba()
            .clone()
            .log_with_msg(DERIVATION_START, Some(self.description()));
        self.scuba.log_with_msg(DERIVATION_START, None);
    }

    /// Log the end of derivation to both the request and derived data scuba
    /// tables.
    pub(super) fn log_derivation_end(
        &mut self,
        ctx: &CoreContext,
        stats: &FutureStats,
        error: Option<&Error>,
    ) {
        let (tag, error_str) = match error {
            None => (DERIVATION_END, None),
            Some(error) => (FAILED_DERIVATION, Some(format!("{:#}", error))),
        };

        let mut ctx_scuba = ctx.scuba().clone();
        ctx_scuba.add_future_stats(stats);
        if let Some(error_str) = &error_str {
            ctx_scuba.add("Derive error", error_str.as_str());
        }
        ctx_scuba.log_with_msg(tag, Some(self.description()));

        ctx.perf_counters().insert_perf_counters(&mut self.scuba);
        self.scuba.add_future_stats(stats);
        self.scuba.log_with_msg(tag, error_str);
    }

    /// Log the start of batch derivation to both the request and derived data
    /// scuba tables.
    pub(super) fn log_batch_derivation_start(&mut self, ctx: &CoreContext) {
        ctx.scuba()
            .clone()
            .log_with_msg(DERIVATION_START_BATCH, Some(self.description()));
        self.scuba.log_with_msg(DERIVATION_START_BATCH, None);
    }

    /// Log the end of derivation to both the request and derived data scuba
    /// tables.
    pub(super) fn log_batch_derivation_end(
        &mut self,
        ctx: &CoreContext,
        stats: &FutureStats,
        error: Option<&Error>,
    ) {
        let (tag, error_str) = match error {
            None => (DERIVATION_END_BATCH, None),
            Some(error) => (FAILED_DERIVATION_BATCH, Some(format!("{:#}", error))),
        };

        let mut ctx_scuba = ctx.scuba().clone();
        ctx_scuba.add_future_stats(stats);
        if let Some(error_str) = &error_str {
            ctx_scuba.add("Derive error", error_str.as_str());
        };
        ctx_scuba.log_with_msg(tag, Some(self.description()));

        ctx.perf_counters().insert_perf_counters(&mut self.scuba);
        self.scuba.add_future_stats(stats);
        self.scuba.log_with_msg(tag, error_str);
    }

    /// Log the start of remote derivation to the derived data scuba table.
    pub(super) fn log_remote_derivation_start(&mut self, ctx: &CoreContext) {
        ctx.scuba()
            .clone()
            .log_with_msg("Requesting remote derivation", Some(self.description()));
        self.scuba
            .log_with_msg("Requesting remote derivation", None);
    }

    /// Log the end of remote derivation to the derived data scuba table.
    pub(super) fn log_remote_derivation_end(&mut self, ctx: &CoreContext, error: Option<String>) {
        let tag = match error {
            None => "Remote derivation finished",
            Some(_) => "Derived data service failed",
        };

        let mut ctx_scuba = ctx.scuba().clone();
        if let Some(error) = error.as_deref() {
            ctx_scuba.add("Derive error", error);
        };
        ctx_scuba.log_with_msg(tag, Some(self.description()));

        ctx.perf_counters().insert_perf_counters(&mut self.scuba);
        self.scuba.log_with_msg(tag, error);
    }

    /// Log the insertion of a new derived data mapping to the derived data
    /// scuba table.
    pub(super) fn log_mapping_insertion(
        &mut self,
        ctx: &CoreContext,
        value: Option<&Derivable>,
        stats: &FutureStats,
        error: Option<&Error>,
    ) {
        let (tag, error_str) = match error {
            None => (INSERTED_MAPPING, None),
            Some(error) => (FAILED_INSERTING_MAPPING, Some(format!("{:#}", error))),
        };

        ctx.perf_counters().insert_perf_counters(&mut self.scuba);

        if let Some(value) = value {
            // Limit how much we log to scuba.
            let value = format!("{:1000?}", value);
            self.scuba.add("mapping_value", value);
        }

        self.scuba
            .add_future_stats(stats)
            .log_with_msg(tag, error_str);
    }
}

impl DerivedDataManager {
    fn should_log_slow_derivation(&self, duration: Duration) -> bool {
        const FALLBACK_THRESHOLD_SECS: u64 = 15;

        let threshold: u64 = justknobs::get_as::<u64>(
            "scm/mononoke_timeouts:derived_data_slow_derivation_threshold_secs",
            None,
        )
        .unwrap_or(FALLBACK_THRESHOLD_SECS);

        duration > Duration::from_secs(threshold)
    }

    /// Log an instance of slow derivation to the request table.
    pub(super) fn log_slow_derivation<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        stats: &FutureStats,
        pc: &PerfCounters,
        result: &Result<DerivationOutcome<Derivable>, DerivationError>,
    ) where
        Derivable: BonsaiDerivable,
    {
        if !self.should_log_slow_derivation(stats.completion_time) {
            return;
        }

        let mut scuba = ctx.scuba().clone();
        pc.insert_perf_counters(&mut scuba);

        scuba.add_future_stats(stats);
        scuba.add("changeset_id", csid.to_string());
        scuba.add("derived_data_type", Derivable::NAME);
        scuba.add("repo", self.repo_name());

        match result {
            Ok(derivation_outcome) => {
                scuba.add("derived", derivation_outcome.count);
                scuba.add(
                    "find_underived_completion_time_ms",
                    derivation_outcome.find_underived_time.as_millis_unchecked(),
                );
                warn!(
                    ctx.logger(),
                    "slow derivation of {} for {}, took {:.2?} (find_underived: {:.2?}), derived {} changesets",
                    Derivable::NAME,
                    csid,
                    stats.completion_time,
                    derivation_outcome.find_underived_time,
                    derivation_outcome.count,
                );
            }
            Err(derivation_error) => {
                warn!(
                    ctx.logger(),
                    "slow derivation of {} for {}, took {:.2?}, failed with Err({:?})",
                    Derivable::NAME,
                    csid,
                    stats.completion_time,
                    derivation_error,
                );
            }
        }

        scuba.log_with_msg(SLOW_DERIVATION, None);
    }
}
