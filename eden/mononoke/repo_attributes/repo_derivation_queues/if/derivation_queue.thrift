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

/// Self-describing stage payload embedded in a queued dag item. The payload is
/// computed from `DerivationPipelineConfig` at enqueue time, so workers do not
/// have to re-read the live config to translate `stage_id` into paths.
union DerivationStagePayload {
  1: ManifestStagePayload manifest;
}

/// Payload for a manifest-style derivation stage. `path` is the absolute path
/// prefix this stage is responsible for; `deps` is the absolute path of each
/// dependency stage. The validator enforces that every dep is exactly one
/// `MPathElement` deeper than the stage path, and that the terminal stage
/// (the one whose output covers the whole repo) is always at `MPath::ROOT` —
/// so workers reconstruct "is terminal" as `path.is_root()` and use
/// `MPath::ROOT` for any cross-stage terminal lookup.
@rust.Exhaustive
struct ManifestStagePayload {
  1: path.MPath path;
  2: list<path.MPathElement> deps;
}

@rust.Exhaustive
struct DagItemInfo {
  1: id.ChangesetId head_cs_id;
  2: optional time.Timestamp enqueue_timestamp;
  3: optional string client_info;
  4: optional i64 bubble_id;
  5: optional i64 retry_count;
  6: DerivationPriority priority;
  7: optional DerivationStagePayload stage_payload;
}
