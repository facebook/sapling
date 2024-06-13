/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/bonsai.thrift"
include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"

typedef id.Sha1 HgNodeHash (rust.newtype)

// Changeset contents are stored inline.
struct HgChangesetEnvelope {
  // The node ID is expected to match the contents exactly.
  1: HgNodeHash node_id;
  2: optional HgNodeHash p1;
  3: optional HgNodeHash p2;
  // These contents are exactly as they would be serialized by Mercurial.
  4: optional binary contents;
} (rust.exhaustive)

// Manifest contents are expected to generally be small, so they're stored
// inline in the envelope. There's also no real dedup possible between native
// Mononoke data structures and these ones.
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
} (rust.exhaustive)

struct HgFileEnvelope {
  1: HgNodeHash node_id;
  2: optional HgNodeHash p1;
  3: optional HgNodeHash p2;
  4: optional id.ContentId content_id;
  // content_size is a u64 stored as an i64, and doesn't include the size of
  // the metadata
  5: i64 content_size;
  6: optional binary metadata;
} (rust.exhaustive)

/// Specification of Augmented Manifest Format

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
} (rust.exhaustive)

struct HgAugmentedDirectoryNode {
  // Identity of the child Mercurial tree node
  1: HgNodeHash treenode;
  // Expected to match the hash of the directory's encoded augmented mf.
  2: id.Blake3 augmented_manifest_id;
  // Expected to match the size of the directory's encoded augmented mf.
  3: i64 augmented_manifest_size;
} (rust.exhaustive)

union HgAugmentedManifestEntry {
  1: HgAugmentedFileLeaf file;
  2: HgAugmentedDirectoryNode directory;
} (rust.exhaustive)

// Augmented HgManifest (core type for traversing content addressed tree manifests via CAS)
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
} (rust.exhaustive)

// Augmented HgManifest Envelope (stored in Mononoke)
struct HgAugmentedManifestEnvelope {
  // Expected to match the hash of the encoded augmented mf.
  1: id.Blake3 augmented_manifest_id;
  // Expected to match the size of the encoded augmented mf.
  2: i64 augmented_manifest_size;
  // HgAugmentedManifest data
  8: HgAugmentedManifest augmented_manifest;
} (rust.exhaustive)
