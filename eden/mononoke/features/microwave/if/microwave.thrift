/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/path.thrift"
include "eden/mononoke/mercurial/types/if/mercurial_thrift.thrift"
include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

// Code version constant -- update to invalidate saved state.
const i32 CODEVER = 1;

@rust.Exhaustive
struct FilenodeSnapshot {
  // Note:  All fields must be present.  They are marked as optional so that we
  // can detect if they are missing.
  1: optional path.RepoPath path;
  2: optional mercurial_thrift.HgNodeHash filenode;
  3: optional mercurial_thrift.HgNodeHash p1;
  4: optional mercurial_thrift.HgNodeHash p2;
  5: optional CopyInfoSnapshot copyfrom;
  6: optional mercurial_thrift.HgNodeHash linknode;
}

@rust.Exhaustive
struct CopyInfoSnapshot {
  1: optional path.RepoPath path;
  2: optional mercurial_thrift.HgNodeHash filenode;
}

@rust.Exhaustive
struct ChangesetSnapshot {
  1: optional id.ChangesetId cs_id;
  2: optional list<id.ChangesetId> parents;
  3: optional i64 gen;
}

@rust.Exhaustive
struct RepoSnapshot {
  1: optional list<FilenodeSnapshot> filenodes;
  2: optional list<ChangesetSnapshot> changesets;
}
