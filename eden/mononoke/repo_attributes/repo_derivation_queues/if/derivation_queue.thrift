/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/path.thrift"
include "eden/mononoke/mononoke_types/serialization/time.thrift"
include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

@rust.Serde
enum DerivationPriority {
  LOW = 0,
  HIGH = 1,
}

/// Stage payload sent by the pipeline tailer to its workers.
union DerivationStagePayload {
  1: ManifestStagePayload manifest;
  2: FinalizeStagePayload finalize;
}

/// Derive `path` using the outputs of its child stages in `deps`.
@rust.Exhaustive
struct ManifestStagePayload {
  1: path.MPath path;
  2: list<path.MPathElement> deps;
}

@rust.Exhaustive
struct FinalizeStagePayload {}

@rust.Exhaustive
struct DagItemInfo {
  1: id.ChangesetId head_cs_id;
  2: optional time.Timestamp enqueue_timestamp;
  3: optional string client_info;
  4: optional i64 bubble_id;
  5: optional i64 retry_count;
  6: DerivationPriority priority;
  // Field 7 (stage_payload) is retired.
}
