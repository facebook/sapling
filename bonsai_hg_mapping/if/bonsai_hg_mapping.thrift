// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

include "scm/mononoke/mercurial/types/if/mercurial_thrift.thrift"
include "scm/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

# Memcache constants. Should be change when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 0
const i32 MC_SITEVER = 0

typedef i32 RepoId (hs.newtype)

struct BonsaiHgMappingEntry {
  1: required RepoId repo_id,
  2: required mononoke_types_thrift.ChangesetId bcs_id,
  3: required mercurial_thrift.HgNodeHash hg_cs_id,
}
