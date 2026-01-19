/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

/// Code version used in memcache keys.  This should be changed whenever
/// the layout of memcache entries is changed in an incompatible way.
/// The corresponding sitever, which can be used to flush memcache, is
/// in the JustKnob scm/mononoke_memcache_sitevers:git_symbolic_refs.
const i32 MC_CODEVER = 0;

@rust.NewType
typedef i32 RepoId

@rust.Exhaustive
struct GitSymbolicRefsCacheEntry {
  1: RepoId repo_id;
  2: string symref_name;
  3: string ref_name;
  4: string ref_type;
}
