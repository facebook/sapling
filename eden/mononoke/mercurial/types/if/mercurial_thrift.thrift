/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/bonsai.thrift"
include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"
include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

@rust.NewType
typedef id.Sha1 HgNodeHash

// Changeset contents are stored inline.
@rust.Exhaustive
struct HgChangesetEnvelope {
  // The node ID is expected to match the contents exactly.
  1: HgNodeHash node_id;
  2: optional HgNodeHash p1;
  3: optional HgNodeHash p2;
  // These contents are exactly as they would be serialized by Mercurial.
  4: optional binary contents;
}

// Manifest contents are expected to generally be small, so they're stored
// inline in the envelope. There's also no real dedup possible between native
// Mononoke data structures and these ones.
@rust.Exhaustive
struct HgManifestEnvelope {
  1: HgNodeHash node_id;
  2: optional HgNodeHash p1;
  3: optional HgNodeHash p2;
  // Root tree manifest nodes can have node IDs that don't match the contents.
  // That is required for lookups, but it means that in the event of recovery
  // from a disaster, hash consistency can't be checked. The computed node ID
  // is stored to allow that to happen.
  4: HgNodeHash computed_node_id;
  // These contents are exactly as they would be serialized by Mercurial.
  5: optional binary contents;
}

@rust.Exhaustive
struct HgFileEnvelope {
  1: HgNodeHash node_id;
  2: optional HgNodeHash p1;
  3: optional HgNodeHash p2;
  4: optional id.ContentId content_id;
  // content_size is a u64 stored as an i64, and doesn't include the size of
  // the metadata
  5: i64 content_size;
  6: optional binary metadata;
}

/// Specification of Augmented Manifest Format

@rust.Exhaustive
struct HgAugmentedFileLeaf {
  // File type (file, link or exec)
  1: bonsai.FileType file_type;
  // Identity of the Mercurial filenode
  2: HgNodeHash filenode;
  // Expected to match the hash of the raw file blob.
  3: id.Blake3 content_blake3;
  // Expected to match the size of the raw blob.
  4: i64 total_size;
  5: id.Sha1 content_sha1;
  // File's metadata blob (that includes "copy from" information)
  6: optional binary file_header_metadata;
}

@rust.Exhaustive
struct HgAugmentedDirectoryNode {
  // Identity of the child Mercurial tree node
  1: HgNodeHash treenode;
  // Expected to match the hash of the directory's encoded augmented mf.
  2: id.Blake3 augmented_manifest_id;
  // Expected to match the size of the directory's encoded augmented mf.
  3: i64 augmented_manifest_size;
  /// Pointer to the AclManifest node for this directory.
  /// Present only if this directory is in the sparse AclManifest.
  4: optional id.AclManifestId acl_manifest_directory_id;
}

union HgAugmentedManifestEntry {
  1: HgAugmentedFileLeaf file;
  2: HgAugmentedDirectoryNode directory;
}

// Augmented HgManifest (core type for traversing content addressed tree manifests via CAS)
@rust.Exhaustive
struct HgAugmentedManifest {
  // Identity of this tree as stored by Mercurial
  1: HgNodeHash hg_node_id;
  // The tree's parents
  2: optional HgNodeHash p1;
  3: optional HgNodeHash p2;
  // Computed id of this tree.
  4: HgNodeHash computed_node_id;
  // Sharded Map of MPathElement -> HgAugmentedManifestEntry
  5: sharded_map.ShardedMapV2Node subentries;
  /// Pointer to this directory's AclManifest node.
  6: optional id.AclManifestId acl_manifest_directory_id;
}

// Augmented HgManifest Envelope (stored in Mononoke)
@rust.Exhaustive
struct HgAugmentedManifestEnvelope {
  // Expected to match the hash of the encoded augmented mf.
  1: id.Blake3 augmented_manifest_id;
  // Expected to match the size of the encoded augmented mf.
  2: i64 augmented_manifest_size;
  // HgAugmentedManifest data
  8: HgAugmentedManifest augmented_manifest;
}

// Per-stage output of the MappedHgChangesetId derivation pipeline.
//
// Mirrors fsnodes_thrift::FsnodeStageOutput but adds a `terminal` variant
// for the root-stage output that carries both the HgChangesetId and the
// root HgManifestId so descendants can hash their envelopes without a
// secondary bonsai_hg_mapping lookup for in-batch parents.

@rust.Exhaustive
struct HgManifestStageOutputEmpty {}

@rust.Exhaustive
struct HgManifestStageTree {
  1: HgNodeHash manifest_id;
}

@rust.Exhaustive
struct HgManifestStageLeaf {
  1: bonsai.FileType file_type;
  2: HgNodeHash filenode_id;
}

// Carries both the terminal stage's HgChangesetId and the manifest entry
// at the stage's path (the root manifest, when the terminal stage is at
// MPath::ROOT), so the terminal-stage parent's hg_cs_id is recoverable
// for envelope hashing of in-batch descendants.
@rust.Exhaustive
struct HgManifestStageTerminal {
  1: HgNodeHash hg_cs_id;
  2: HgNodeHash manifest_id;
}

union HgManifestStageOutput {
  1: HgManifestStageTree tree;
  2: HgManifestStageLeaf leaf;
  3: HgManifestStageOutputEmpty empty;
  4: HgManifestStageTerminal terminal;
}

// Per-stage output of the RootHgAugmentedManifestId derivation pipeline.
//
// Mirrors fsnodes_thrift::FsnodeStageOutput: the two non-empty arms reuse
// HgAugmentedManifestEntry's thrift (a DirectoryNode at a tree stage, a
// FileNode when the stage root is a file); `empty` means nothing exists at
// the stage path.

@rust.Exhaustive
struct HgAugmentedManifestStageOutputEmpty {}

union HgAugmentedManifestStageOutput {
  1: HgAugmentedDirectoryNode directory;
  2: HgAugmentedFileLeaf file;
  3: HgAugmentedManifestStageOutputEmpty empty;
}
