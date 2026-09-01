/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Per-push merge-resolution outcome carried on `PushrebaseOutcome` and
//! logged on per-push Scuba completion samples. Lets MR effectiveness be
//! analyzed from a single sample without joining the sibling
//! "Pushrebase merge resolution succeeded/failed" log lines.

use mercurial_types::NonRootMPath;
use scuba_ext::MononokeScubaSampleBuilder;
use strum::IntoStaticStr;

/// Cap on the number of resolved paths included in
/// `MergeResolutionSummary::Succeeded::resolved_paths_sample`. A codemod
/// commit can touch tens of thousands of paths; sampling keeps the Scuba
/// payload bounded while still letting analysts identify the file family
/// that was resolved.
pub const MR_PATH_SAMPLE_CAP: usize = 64;

/// Per-push merge-resolution summary. Each variant carries exactly the
/// fields meaningful for that outcome — combinations like `Succeeded` with
/// no resolved files, or `NotNeeded` with a non-zero conflict count, are
/// unrepresentable. The `IntoStaticStr` derive provides the stable Scuba
/// bucket literal via `outcome_str()`.
#[derive(Debug, Clone, IntoStaticStr)]
pub enum MergeResolutionSummary {
    /// No file conflicts; merge resolution was never invoked.
    #[strum(serialize = "not_needed")]
    NotNeeded,

    /// MR resolved every conflicting file.
    #[strum(serialize = "succeeded")]
    Succeeded {
        conflict_files_count: u64,
        resolved_files_count: u64,
        /// Capped at `MR_PATH_SAMPLE_CAP`. May be shorter than
        /// `resolved_files_count` for codemod-scale resolutions.
        resolved_paths_sample: Vec<NonRootMPath>,
    },

    /// MR rejected: more conflicting files than `pushrebase_max_merge_conflicts`.
    #[strum(serialize = "failed_too_many_conflicts")]
    FailedTooManyConflicts { conflict_files_count: u64 },

    /// MR skipped: path-prefix excluded, type mismatch, copy info, file
    /// too large, not a tracked change, prefix conflict, fsnodes not
    /// derived, etc.
    #[strum(serialize = "failed_skipped")]
    FailedSkipped {
        conflict_files_count: u64,
        detail: String,
    },

    /// MR hit an internal blobstore/derivation error.
    #[strum(serialize = "failed_internal")]
    FailedInternal {
        conflict_files_count: u64,
        detail: String,
    },
}

impl MergeResolutionSummary {
    /// Stable snake_case Scuba bucket literal. Never rename a variant's
    /// `#[strum(serialize = ...)]` value without coordinating with
    /// downstream dashboards/alerts that bucket on `mr_outcome`.
    pub fn outcome_str(&self) -> &'static str {
        self.into()
    }

    /// Number of conflicting paths reported by `intersect_changed_files`,
    /// regardless of whether MR was attempted. 0 for `NotNeeded`.
    pub fn conflict_files_count(&self) -> u64 {
        match self {
            Self::NotNeeded => 0,
            Self::Succeeded {
                conflict_files_count,
                ..
            }
            | Self::FailedTooManyConflicts {
                conflict_files_count,
            }
            | Self::FailedSkipped {
                conflict_files_count,
                ..
            }
            | Self::FailedInternal {
                conflict_files_count,
                ..
            } => *conflict_files_count,
        }
    }

    /// Number of files MR successfully resolved. Non-zero only for `Succeeded`.
    pub fn resolved_files_count(&self) -> u64 {
        match self {
            Self::Succeeded {
                resolved_files_count,
                ..
            } => *resolved_files_count,
            _ => 0,
        }
    }

    /// Construct a `Succeeded` summary representing a prior MR attempt
    /// whose original conflict count is no longer available — only the
    /// resolved paths survived in CAS-failure-retry carry-forward state.
    /// `conflict_files_count` is reported as 0; analysts wanting the
    /// original count can join on the per-attempt
    /// "Pushrebase merge resolution succeeded" Scuba sample.
    pub fn from_carried_paths(paths: Vec<NonRootMPath>) -> Self {
        let resolved_files_count = paths.len() as u64;
        let resolved_paths_sample = paths.into_iter().take(MR_PATH_SAMPLE_CAP).collect();
        Self::Succeeded {
            conflict_files_count: 0,
            resolved_files_count,
            resolved_paths_sample,
        }
    }

    /// Combine two summaries from disjoint conflict-check ranges
    /// (speculative range outside the lock + delta range inside; or
    /// across CAS-failure retries). `Succeeded` is sticky: if either
    /// side succeeded the combined result is `Succeeded` with summed
    /// counts and concatenated paths capped at `MR_PATH_SAMPLE_CAP`.
    /// Otherwise the more recent (delta) outcome wins.
    pub fn combine(speculative: Self, delta: Self) -> Self {
        use MergeResolutionSummary::*;
        match (speculative, delta) {
            (
                Succeeded {
                    conflict_files_count: sc,
                    resolved_files_count: sr,
                    resolved_paths_sample: sp,
                },
                Succeeded {
                    conflict_files_count: dc,
                    resolved_files_count: dr,
                    resolved_paths_sample: dp,
                },
            ) => Succeeded {
                conflict_files_count: sc + dc,
                resolved_files_count: sr + dr,
                resolved_paths_sample: sp.into_iter().chain(dp).take(MR_PATH_SAMPLE_CAP).collect(),
            },
            // Succeeded on either side stays sticky.
            (s @ Succeeded { .. }, _) => s,
            (_, d @ Succeeded { .. }) => d,
            // No success on either side: the more recent (delta) outcome wins.
            (_, d) => d,
        }
    }

    /// Append this summary's fields to the given Scuba sample. Always
    /// emits `mr_outcome` + `mr_conflict_files_count` +
    /// `mr_resolved_files_count`. Variant-specific fields
    /// (`mr_resolved_paths_sample`, `mr_failure_detail`) are emitted only
    /// when meaningful for the variant.
    pub fn add_to_scuba(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("mr_outcome", self.outcome_str());
        scuba.add(
            "mr_conflict_files_count",
            self.conflict_files_count() as i64,
        );
        scuba.add(
            "mr_resolved_files_count",
            self.resolved_files_count() as i64,
        );
        match self {
            Self::Succeeded {
                resolved_paths_sample,
                ..
            } if !resolved_paths_sample.is_empty() => {
                let stringified: Vec<String> = resolved_paths_sample
                    .iter()
                    .map(|p| p.to_string())
                    .collect();
                scuba.add("mr_resolved_paths_sample", stringified);
            }
            Self::FailedSkipped { detail, .. } | Self::FailedInternal { detail, .. } => {
                scuba.add("mr_failure_detail", detail.as_str());
            }
            _ => {}
        }
    }
}
