// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

include "scm/mononoke/mononoke-types/if/mononoke_types_thrift.thrift"

typedef mononoke_types_thrift.Sha1 HgNodeHash (hs.newtype)

struct HgFileEnvelope {
  1: required HgNodeHash node_id,
  2: optional HgNodeHash p1,
  3: optional HgNodeHash p2,
  4: optional mononoke_types_thrift.ContentId content_id,
  // content_size is a u64 stored as an i64, and doesn't include the size of
  // the metadata
  5: required i64 content_size,
  6: optional binary metadata,
}
