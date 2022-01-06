/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"
include "eden/mononoke/mercurial/types/if/mercurial_thrift.thrift"

// Code version constant -- update to invalidate saved state.
const i32 CODEVER = 1;

struct FilenodeSnapshot {
  // Note: required fields are enforced at runtime here (to prevent Thift from
  // giving us garbage values and calling those acceptable).
  1: optional mononoke_types_thrift.RepoPath path;
  2: optional mercurial_thrift.HgNodeHash filenode;
  3: optional mercurial_thrift.HgNodeHash p1;
  4: optional mercurial_thrift.HgNodeHash p2;
  5: optional CopyInfoSnapshot copyfrom;
  6: optional mercurial_thrift.HgNodeHash linknode;
} (rust.exhaustive)

struct CopyInfoSnapshot {
  1: optional mononoke_types_thrift.RepoPath path;
  2: optional mercurial_thrift.HgNodeHash filenode;
} (rust.exhaustive)

struct ChangesetSnapshot {
  1: optional mononoke_types_thrift.ChangesetId cs_id;
  2: optional list<mononoke_types_thrift.ChangesetId> parents;
  3: optional i64 gen;
} (rust.exhaustive)

struct RepoSnapshot {
  1: optional list<FilenodeSnapshot> filenodes;
  2: optional list<ChangesetSnapshot> changesets;
} (rust.exhaustive)
