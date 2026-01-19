/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mercurial/types/if/mercurial_thrift.thrift"
include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

/// Code version used in memcache keys.  This should be changed whenever
/// the layout of memcache entries is changed in an incompatible way.
/// The corresponding sitever, which can be used to flush memcache, is
/// in the JustKnob scm/mononoke_memcache_sitevers:bonsai_hg_mapping.
const i32 MC_CODEVER = 0;

@rust.NewType
typedef i32 RepoId

@rust.Exhaustive
struct BonsaiHgMappingCacheEntry {
  1: RepoId repo_id;
  2: id.ChangesetId bcs_id;
  3: mercurial_thrift.HgNodeHash hg_cs_id;
}
