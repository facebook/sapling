/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! ------------
//! IMPORTANT!!!
//! ------------
//! Do not change the order of the fields! Changing the order of the fields
//! results in compatible but *not* identical serializations, so hashes will
//! change.
//! ------------
//! IMPORTANT!!!
//! ------------

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"
include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

/// Whether a directory itself is restricted
union AclManifestDirectoryRestriction {
  /// Directory is not restricted (waypoint to reach restriction roots)
  1: AclManifestUnrestricted unrestricted;
  /// Directory is a restriction root
  2: AclManifestRestriction restricted;
}

@rust.Exhaustive
struct AclManifestUnrestricted {}

/// A restriction root — points to a separately stored ACL entry blob
@rust.Exhaustive
struct AclManifestRestriction {
  /// ID of the content-addressed AclManifestEntryBlob in the blobstore
  1: id.AclManifestEntryBlobId entry_blob_id;
}

/// A child directory in the parent's ShardedMapV2.
@rust.Exhaustive
struct AclManifestDirectoryEntry {
  /// ID of the child's AclManifest node
  1: id.AclManifestId id;
  /// Whether this directory is a restriction root
  2: bool is_restricted;
  /// Whether any descendant of this directory is a restriction root
  3: bool has_restricted_descendants;
}

/// Entry in the ShardedMapV2 — either an ACL file (leaf) or a subdirectory (tree).
union AclManifestEntry {
  /// A .slacl file in this directory
  1: AclManifestRestriction acl_file;
  /// A subdirectory that is a restriction root or waypoint
  2: AclManifestDirectoryEntry directory;
}

/// A node in the sparse ACL manifest tree.
/// Only exists for restriction roots and their ancestors.
@rust.Exhaustive
struct AclManifest {
  /// Whether THIS directory is a restriction root.
  /// Stored on the node so pointer-based lookups can read it directly.
  1: AclManifestDirectoryRestriction restriction;
  /// Children that are restriction roots or waypoints.
  2: sharded_map.ShardedMapV2Node subentries;
}

/// Rollup data for ShardedMapV2 pruning
@rust.Exhaustive
struct AclManifestRollup {
  /// True if any Directory entry exists in this subtree of the sharded map.
  /// AclFile (leaf) entries are excluded — only directories representing
  /// restriction roots or waypoints count.
  1: bool has_restricted;
}

/// Individually-stored ACL metadata for a restriction root.
/// Content-addressed: identical .slacl files share the same blob.
@rust.Exhaustive
struct AclManifestEntryBlob {
  /// REPO_REGION ACL protecting this directory
  /// e.g. "repos/hg/fbsource/=project1"
  1: string repo_region_acl;
  /// AMP group to direct users to for access requests.
  /// Defaults to repo_region_acl if not set.
  2: optional string permission_request_group;
}
