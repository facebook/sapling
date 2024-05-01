/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/time.thrift"

struct DagItemInfo {
  1: id.ChangesetId head_cs_id;
  2: optional time.Timestamp enqueue_timestamp;
  3: optional string client_info;
} (rust.exhaustive)
