/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

include "scm/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

struct BlobHandle {
  1: mononoke_types_thrift.GitSha1 oid,
  2: i64 size,
  3: mononoke_types_thrift.FileType file_type,
}

struct TreeHandle {
  1: mononoke_types_thrift.GitSha1 oid,
  2: i64 size,
}

union TreeMember {
  1: BlobHandle Blob,
  2: TreeHandle Tree,
}

struct Tree {
  1: TreeHandle handle,
  2: map<mononoke_types_thrift.MPathElement, TreeMember> members,
}
