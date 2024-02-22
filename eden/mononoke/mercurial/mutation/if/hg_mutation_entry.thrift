/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mercurial/types/if/mercurial_thrift.thrift"

/// Additional property that can be associated with some data type.
struct ExtraProperty {
  1: string key;
  2: string value;
} (rust.exhaustive)

/// Record of a Mercurial mutation operation (e.g. amend or rebase).
struct HgMutationEntry {
  /// The commit that resulted from the mutation operation.
  1: mercurial_thrift.HgNodeHash successor;
  /// The commits that were mutated to create the successor.
  ///
  /// There may be multiple predecessors, e.g. if the commits were folded.
  2: list<mercurial_thrift.HgNodeHash> predecessors;
  /// Other commits that were created by the mutation operation splitting the predecessors.
  ///
  /// Where a commit is split into two or more commits, the successor will be the final commit,
  /// and this list will contain the other commits.
  3: list<mercurial_thrift.HgNodeHash> split;
  /// The name of the operation.
  4: string op;
  /// The user who performed the mutation operation. This may differ from the commit author.
  5: string user;
  /// The timestamp of the mutation operation. This may differ from the commit time.
  6: i64 timestamp;
  /// The timezone offset of the mutation operation. This may differ from the commit time.
  7: i32 timezone;
  /// Extra information about this mutation operation.
  8: list<ExtraProperty> extra;
} (rust.exhaustive)

/// Code version used in memcache keys.  This should be changed whenever
/// the layout of memcache entries is changed in an incompatible way.
/// The corresponding sitever, which can be used to flush memcache, is
/// in the JustKnob scm/mononoke_memcache_sitevers:hg_mutation_store.
const i32 MC_CODEVER = 0;

typedef i32 RepoId (rust.newtype)

/// Struct corresponding to a mutation records scoped to a repository and
/// changeset ID
struct HgMutationCacheEntry {
  /// The mutation entries that are part of this cache record
  1: list<HgMutationEntry> mutation_entries;
  /// The ID of the repository corresponding to the mutation entries
  2: RepoId repo_id;
  /// The ID of the changeset corresponding to the mutation entries
  3: mercurial_thrift.HgNodeHash changeset_id;
} (rust.exhaustive)
