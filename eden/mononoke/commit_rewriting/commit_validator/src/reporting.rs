/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cross_repo_sync::types::Large;
use cross_repo_sync::types::Small;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use scuba_ext::MononokeScubaSampleBuilder;
use std::time::Duration;

use crate::tail::QueueSize;

const LARGE_REPO: &str = "large_repo";
const SMALL_REPO: &str = "small_repo";
const LARGE_CS_ID: &str = "large_cs_id";
const SMALL_CS_ID: &str = "small_cs_id";
const NOOP_ITERATION: &str = "noop";
const ENTRY: &str = "entry";
const VALIDATION_DURATION_MS: &str = "validation_duration_ms";
const PREPARATION_DURATION_MS: &str = "preparation_duration_ms";
const QUEUE_SIZE: &str = "queue_size";
const ERROR: &str = "error";
const SUCCESS: &str = "success";

pub fn add_common_commit_syncing_fields(
    scuba_sample: &mut MononokeScubaSampleBuilder,
    large_repo_id: Large<RepositoryId>,
    small_repo_id: Small<RepositoryId>,
) {
    scuba_sample
        .add(LARGE_REPO, large_repo_id.0.id())
        .add(SMALL_REPO, small_repo_id.0.id());
}

pub fn log_validation_result_to_scuba(
    mut scuba_sample: MononokeScubaSampleBuilder,
    entry_id: i64,
    large_cs_id: &Large<ChangesetId>,
    small_cs_id: &Small<ChangesetId>,
    error: Option<String>,
    queue_size: QueueSize,
    preparation_duration: Duration,
    validation_duration: Duration,
) {
    scuba_sample
        .add(LARGE_CS_ID, format!("{}", large_cs_id))
        .add(SMALL_CS_ID, format!("{}", small_cs_id))
        .add(NOOP_ITERATION, 0)
        .add(QUEUE_SIZE, queue_size.0)
        .add(
            VALIDATION_DURATION_MS,
            validation_duration.as_millis() as u64,
        )
        .add(
            PREPARATION_DURATION_MS,
            preparation_duration.as_millis() as u64,
        )
        .add(ENTRY, entry_id);
    match error {
        Some(error) => {
            scuba_sample.add(ERROR, error).add(SUCCESS, 0);
        }
        None => {
            scuba_sample.add(SUCCESS, 1);
        }
    };

    scuba_sample.log();
}

pub fn log_noop_iteration_to_scuba(
    mut scuba_sample: MononokeScubaSampleBuilder,
    large_repo_id: RepositoryId,
) {
    scuba_sample
        .add(NOOP_ITERATION, 1)
        .add(LARGE_REPO, large_repo_id.id())
        .add(SUCCESS, 1)
        .add(QUEUE_SIZE, 0);
    scuba_sample.log();
}
