/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mercurial/types/if/mercurial_thrift.thrift"

/// Additional property that can be associated with some data type.
struct ExtraProperty {
  1: required string key;
  2: required string value;
} (rust.exhaustive)

/// Record of a Mercurial mutation operation (e.g. amend or rebase).
struct HgMutationEntry {
  /// The commit that resulted from the mutation operation.
  1: required mercurial_thrift.HgNodeHash successor;
  /// The commits that were mutated to create the successor.
  ///
  /// There may be multiple predecessors, e.g. if the commits were folded.
  2: required list<mercurial_thrift.HgNodeHash> predecessors;
  /// Other commits that were created by the mutation operation splitting the predecessors.
  ///
  /// Where a commit is split into two or more commits, the successor will be the final commit,
  /// and this list will contain the other commits.
  3: list<mercurial_thrift.HgNodeHash> split;
  /// The name of the operation.
  4: required string op;
  /// The user who performed the mutation operation. This may differ from the commit author.
  5: required string user;
  /// The timestamp of the mutation operation. This may differ from the commit time.
  6: required i64 timestamp;
  /// The timezone offset of the mutation operation. This may differ from the commit time.
  7: required i32 timezone;
  /// Extra information about this mutation operation.
  8: list<ExtraProperty> extra;
} (rust.exhaustive)

# Memcache constants. Should be change when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 0;
const i32 MC_SITEVER = 3;

typedef i32 RepoId (rust.newtype)

/// Struct corresponding to a mutation records scoped to a repository and
/// changeset ID
struct HgMutationCacheEntry {
  /// The mutation entries that are part of this cache record
  1: required list<HgMutationEntry> mutation_entries;
  /// The ID of the repository corresponding to the mutation entries
  2: required RepoId repo_id;
  /// The ID of the changeset corresponding to the mutation entries
  3: required mercurial_thrift.HgNodeHash changeset_id;
} (rust.exhaustive)
