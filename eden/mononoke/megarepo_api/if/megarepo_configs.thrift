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

namespace cpp2 facebook.scm.service
namespace py3 scm.service.thrift
namespace py scm.service.thrift.megarepo_configs
namespace php SourceControlMegarepoConfigStructs
namespace java.swift com.facebook.scm.megarepo_configs

// Megarepo service structs

typedef i64 RepoId
typedef string BookmarkName
typedef string Path
typedef string Prefix
typedef string SyncConfigVersion
typedef binary ChangesetId

/// Source revisions we are interested in
union SourceRevision {
  /// Source is pinned to a given changeset
  1: ChangesetId hash;
  /// Source is tracking a bookmark
  2: BookmarkName bookmark;
}

/// How to remap paths in a given source
@rust.Exhaustive
struct SourceMappingRules {
  /// If no other rule matches, prepend this prefix
  /// to the source path when rewriting
  1: Prefix default_prefix;
  /// Mapping from link name to a target
  3: map<Path, Path> linkfiles;
  /// Paths for which default behavior is overridden
  /// - if a path maps to an empty list, anything
  ///   starting with it is skipped while rewriting
  ///   into a target repo
  /// - if a path maps to multiple items, many files
  ///   will be created in the target repo, with the
  ///   same contents as the original file
  4: map<Prefix, list<Prefix>> overrides;
}

/// Squash side branch of the merge if:
///   1: all commits from the same author
///   2: number of commits in branch less than a limit
@rust.Exhaustive
struct Squashed {
  /// limit for commits in side branch to be squashed
  1: i64 squash_limit;
}

/// Existing merge mode where we create a move commit on top of side branch
@rust.Exhaustive
struct WithExtraMoveCommit {}

/// Enum defines how merges should be handled
union MergeMode {
  /// squash side branch
  1: Squashed squashed;
  /// create extra move commit
  2: WithExtraMoveCommit with_move_commit;
}

/// Synchronization source
@rust.Exhaustive
struct Source {
  /// A name to match sources across version bumps
  /// Has no meaning, except for book-keeping
  1: string source_name;
  /// Mononoke repository id, where source is located
  2: RepoId repo_id;
  /// Name of the original (git) repo, from which this source comes
  3: string name;
  /// Source revisions, from where sync happens
  4: SourceRevision revision;
  /// Rules of commit sync
  5: SourceMappingRules mapping;
  /// How merges should be handled
  6: optional MergeMode merge_mode;
}

/// Synchronization target
@rust.Exhaustive
struct Target {
  /// Mononoke repository id, where the target is located
  1: RepoId repo_id;
  /// Bookmark, which this target represents
  2: BookmarkName bookmark;
}

/// A single version of synchronization config for a target,
/// bundling together all of the corresponding sources
@rust.Exhaustive
struct SyncTargetConfig {
  // A target to which this config can apply
  1: Target target;
  // All of the sources to sync from
  2: list<Source> sources;
  // The version of this config
  3: SyncConfigVersion version;
}
