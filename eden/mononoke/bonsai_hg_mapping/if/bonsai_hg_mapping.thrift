/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mercurial/types/if/mercurial_thrift.thrift"
include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

/// Code version used in memcache keys.  This should be changed whenever
/// the layout of memcache entries is changed in an incompatible way.
/// The corresponding sitever, which can be used to flush memcache, is
/// in the JustKnob scm/mononoke_memcache_sitevers:bonsai_hg_mapping.
const i32 MC_CODEVER = 0;

typedef i32 RepoId (rust.newtype)

struct BonsaiHgMappingCacheEntry {
  1: required RepoId repo_id;
  2: required mononoke_types_thrift.ChangesetId bcs_id;
  3: required mercurial_thrift.HgNodeHash hg_cs_id;
} (rust.exhaustive)
