/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

include "scm/mononoke/mercurial/types/if/mercurial_thrift.thrift"
include "scm/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

# Memcache constants. Should be change when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 0
const i32 MC_SITEVER = 1

typedef i32 RepoId (hs.newtype)

struct BonsaiHgMappingEntry {
  1: required RepoId repo_id,
  2: required mononoke_types_thrift.ChangesetId bcs_id,
  3: required mercurial_thrift.HgNodeHash hg_cs_id,
}
