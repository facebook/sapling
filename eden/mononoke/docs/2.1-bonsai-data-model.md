# Bonsai Data Model

This document explains Mononoke's core data model—the Bonsai format—which serves as the canonical, VCS-agnostic representation of repository data. Understanding Bonsai is essential for working with Mononoke, as it forms the foundation upon which all other repository operations are built.

## Introduction

Bonsai is Mononoke's internal representation of version control data. Every commit in Mononoke, regardless of whether it originated from Git, Mercurial, or Sapling, is stored internally as a Bonsai changeset. This unified format allows Mononoke to maintain a single source of truth while serving multiple client types.

## Inherent Data vs. Derived Data

Mononoke distinguishes between two fundamental categories of data:

**Inherent data** constitutes the core Merkle DAG and serves as the source of truth. This data forms the basis for content-addressed hashing and must be sufficient to represent the entire state and history of the repository. In Mononoke, inherent data consists of:

- Bonsai changesets
- File content blobs
- Original VCS bytes (for Git and Mercurial commits, preserved for compatibility)

**Derived data** comprises indexes and representations computed from inherent data. This data is not included in content-addressed hashes and can be regenerated from inherent data. Derived data enables efficient operations that would be impractical using only the minimal Bonsai representation.

This separation is fundamental to Mononoke's architecture. The write path stores only inherent data, keeping the critical section minimal. Derived data is computed asynchronously off the critical path, allowing Mononoke to maintain high write throughput while still providing efficient read operations.

## Bonsai Changesets

A Bonsai changeset represents a single commit. The structure is defined in `mononoke_types/src/bonsai_changeset.rs` and contains the following components:

### Metadata Fields

#### Parents
- A list of parent changeset identifiers

#### Author Information
- Author name and email
- Author date (when the commit was authored)

#### Committer Information
- Committer name and email (optional, used primarily for Git compatibility)
- Committer date (optional, may differ from author date in Git workflows)

#### Commit Message
- The textual description of the change

#### VCS-Specific Extra Fields
- `hg_extra`: Key-value pairs for Mercurial-specific metadata
- `git_extra_headers`: Key-value pairs for Git-specific headers
- `git_tree_hash`: Not used (deprecated)
- `git_annotated_tag`: Optional Git annotated tag information

These fields allow Bonsai to preserve the complete semantics of both Git and Mercurial commits while maintaining a unified structure.

### File Changes

File changes are represented as a flat list of path-to-change mappings. Each entry specifies the full path to a file and the change that occurred. This differs from Git's nested tree structure and Mercurial's manifest system.

Changes can be:

**Tracked Changes**
- File additions or modifications
- Includes content identifier, file type, size, and optional copy-from information
- Copy-from information explicitly records file copies and renames, referencing the source path and changeset
- Git LFS field controls whether the file should be served as a Git LFS pointer when accessed via Git protocol

**Tracked Deletions**
- Records that a file was removed

**Untracked Changes and Deletions**
- Used for snapshot commits and working directory state that is not part of normal history

#### File Type Information
- Regular file
- Executable file
- Symbolic link
- Git submodule

The file change types are defined in `mononoke_types/src/file_change.rs`.

### Snapshot and Subtree Support

**Snapshot State**
- Indicates whether this changeset represents a snapshot of working directory state
- Snapshots may include untracked changes

**Subtree Changes**
- Represent metadata for copies or merges that apply to subtrees.  This is used in directory branching.

## Content Addressing

Bonsai uses Blake2b hashing to create a content-addressed Merkle DAG. Blake2b produces 256-bit (32-byte) hashes and provides cryptographic strength while being faster than SHA-256.

### Hash Computation

**Changeset Identifiers**
A Bonsai changeset is serialized (using Thrift compact protocol) and hashed with Blake2b. The resulting hash becomes the changeset identifier (`ChangesetId`). This identifier depends on:
- All metadata fields (author, dates, message, etc.)
- Parent changeset hashes
- All file changes (paths, content identifiers, types)
- Extra fields

Any modification to the changeset produces a different identifier.

**Content Identifiers**
File contents are hashed separately. Each file's content is hashed with Blake2b to produce a `ContentId`. Identical files across different commits or repositories produce the same content identifier, enabling deduplication.

**Merkle DAG Properties**
The parent references in each changeset create a directed acyclic graph. The content-addressed nature ensures:
- Tampering detection (any change produces a different hash)
- Efficient comparison (identical hashes mean identical content)
- Deduplication (identical content stored once)

The hash types are defined in `mononoke_types/src/hash.rs` and `mononoke_types/src/typed_hash.rs`.

## File Content Storage

File contents are stored separately from changesets in the blobstore. This separation provides several characteristics:

**Content Blobs**
- Stored using their `ContentId` as the key
- Immutable once written
- Shared across all changesets that reference them

**Chunking for Large Files**
File content can be stored in chunks for large files. This is managed by the filestore, which handles:
- Splitting large files into manageable chunks
- Reassembling chunks when retrieving files
- Optimizing transfer and storage of large binary files

The `FileContents` type (defined in `mononoke_types/src/file_contents.rs`) can represent either:
- Direct bytes for small to medium files
- References to content chunks for large files

## VCS-Agnostic Design

Bonsai serves as an intermediate representation between different version control systems. The format is designed to capture the semantics of both Git and Mercurial while avoiding the implementation details of either.

### Representation Capabilities

**Git Compatibility**
Bonsai can represent all Git commit information:
- Author and committer as separate entities (matching Git's model)
- Committer date distinct from author date
- Git-specific headers preserved in `git_extra_headers
- Support for Git annotated tags

**Mercurial Compatibility**
Bonsai can represent all Mercurial changeset information:
- Author (Mercurial typically has only author, not separate committer)
- Mercurial extra fields preserved in `hg_extra`
- Copy-from information for file renames

**Simplified Structure**
Unlike both Git and Mercurial, Bonsai uses:
- Flat file change lists rather than nested tree structures
- Explicit copy-from information rather than heuristic rename detection
- Uniform handling of all file changes in a single structure

### Conversion and Mapping

Bonsai changesets are converted to and from VCS-specific formats:

**Git Conversion**
- Git commits are imported by parsing the Git commit object and creating a corresponding Bonsai changeset
- Git trees are converted to the flat Bonsai file change list
- When serving Git clients, Bonsai changesets are converted back to Git commits and trees
- The `bonsai_git_mapping` table maintains bidirectional mappings between `ChangesetId` and Git commit SHA-1

**Mercurial Conversion**
- Mercurial changesets are imported by extracting metadata and file changes
- When serving Mercurial clients, derived data (Mercurial manifests and filenodes) provides the VCS-specific representation
- The `bonsai_hg_mapping` table maintains bidirectional mappings between `ChangesetId` and Mercurial changeset hash
- Original Mercurial changeset bytes are preserved as inherent data for exact reproduction

These conversions allow Mononoke to serve Git and Mercurial clients from a single Bonsai backend while maintaining compatibility with each VCS.

## Immutability and Storage

Bonsai changesets are immutable. Once a changeset is created and stored:

**Write-Once Semantics**
- Changesets are never modified after creation
- The content-addressed identifier ensures any change produces a different changeset
- Corrections require creating new changesets

**Blobstore Storage**
Bonsai changesets are stored in the blobstore:
- Serialized using Thrift compact protocol
- Stored with a blobstore key derived from the `ChangesetId`
- Retrieved by key when needed
- Subject to blobstore caching and multiplexing layers

**Metadata Database References**
While changesets are immutable, mutable state is maintained separately:
- The commit graph index (in the metadata database) tracks parent-child relationships for efficient queries
- VCS mapping tables connect Bonsai identifiers to Git and Mercurial identifiers
- Bookmarks (branch pointers) reference changeset identifiers but can be moved

This separation of immutable content from mutable references allows efficient repository operations while maintaining data integrity.

## Comparison with Git and Mercurial

Understanding how Bonsai differs from Git and Mercurial clarifies the design choices:

### Structure Differences

**Git Trees vs. Bonsai File Changes**
Git represents repository state using nested tree objects. Each tree contains references to blobs (files) and subtrees (subdirectories), forming a recursive structure. Bonsai uses a flat list of file changes, where each entry specifies the complete path from the repository root. This eliminates the need to create and track intermediate tree objects.

**Mercurial Manifests vs. Bonsai File Changes**
Mercurial uses manifest objects that, like Git trees, can be nested. Additionally, Mercurial maintains filelogs (per-file history) with linknodes connecting file revisions to changesets. Bonsai represents only what changed in each commit, with file history derived from the changeset graph.

**Rename and Copy Tracking**
Git does not explicitly track renames or copies. Instead, Git detects renames heuristically when comparing trees. Mercurial explicitly records copy-from information in changeset metadata. Bonsai follows Mercurial's approach, explicitly storing copy-from information as part of tracked file changes.

### Hash Algorithm

**Git**: Uses SHA-1 (40 hex characters) for all object identifiers. Git is transitioning to SHA-256 (64 hex characters).

**Mercurial**: Uses SHA-1 (40 hex characters) for changeset and manifest identifiers.

**Bonsai**: Uses Blake2b (64 hex characters for 256-bit hashes). Blake2b provides cryptographic strength comparable to SHA-256 with better performance.

### Metadata Handling

**Git** distinguishes between author (who wrote the code) and committer (who committed it), each with their own timestamp. This supports workflows where commits are authored by one person and applied by another.

**Mercurial** typically has only an author and date, though extra fields can store additional metadata.

**Bonsai** includes both author and optional committer fields, accommodating Git's model while allowing Mercurial commits (which typically have no committer) to be represented naturally.

## Type Definitions and Implementation

The Bonsai data structures are defined in the `mononoke_types` crate, specifically:

**Core Types** (`mononoke_types/src/bonsai_changeset.rs`)
- `BonsaiChangeset` - The immutable changeset structure
- `BonsaiChangesetMut` - The mutable builder used during changeset creation

**File Changes** (`mononoke_types/src/file_change.rs`)
- `FileChange` - Enumeration of change types (tracked change, deletion, untracked change, untracked deletion)
- `TrackedFileChange` - A file addition or modification with copy-from information
- `BasicFileChange` - File change without copy-from tracking
- `FileType` - File type (regular, executable, symlink)

**Content Addressing** (`mononoke_types/src/typed_hash.rs`)
- `ChangesetId` - Blake2b hash identifying a changeset
- `ContentId` - Blake2b hash identifying file contents

**File Contents** (`mononoke_types/src/file_contents.rs`)
- `FileContents` - Either direct bytes or chunked content
- `ChunkedFileContents` - References to content chunks for large files

**Paths** (`mononoke_types/src/path.rs`)
- `MPath` - A path within the repository (may include root)
- `NonRootMPath` - A path guaranteed not to be the root

Additional types for dates, extra fields, and subtree changes are defined in their respective files within `mononoke_types/src/`.

## Relationship to Derived Data

While Bonsai changesets contain the core commit information, many repository operations require additional data structures. These are provided by derived data, which is computed from Bonsai changesets:

**Manifests**
- `fsnodes` - Filesystem-like directory structure
- `unodes` - Full directory structure with file and directory history.  Similar to Mercurial's manifests but with the "linknode problem" fixed.
- Skeleton manifests - Path-only directory structure for testing existence
- Git trees - Git-compatible tree objects

**File Metadata**
- Filenodes - Per-file history information for Mercurial
- Blame - Line-by-line authorship attribution
- Fastlog - Optimized file history

**VCS-Specific Formats**
- Git commits - Git commit objects for serving Git clients
- Git delta manifests - Precomputed Git deltas for efficient computation of pack files when serving clients
- Mercurial augmented manifests - Mecurial manifests with additional metadata for CASC

These derived data types are described in detail in the Derived Data documentation.

The separation between Bonsai (inherent data) and these structures (derived data) is a fundamental architectural decision. Bonsai provides the minimal representation needed for accepting pushes at high speed, while derived data provides the indexes and formats needed for efficient operations and VCS compatibility.

## Working with Bonsai in Code

When working with Mononoke code, Bonsai changesets are accessed through repository facets:

**Reading Changesets**
The `repo_blobstore` facet provides access to the blobstore, allowing changesets to be loaded by their `ChangesetId`. The `Loadable` trait (implemented for `ChangesetId`) enables loading the corresponding `BonsaiChangeset` from storage.

**Creating Changesets**
New Bonsai changesets are typically created using `BonsaiChangesetMut`, which provides a builder pattern for assembling the changeset components. Once complete, the mutable changeset is frozen into an immutable `BonsaiChangeset` and stored.

**Commit Graph Traversal**
The `commit_graph` facet provides efficient parent-child traversal without loading full changesets. This enables operations like ancestry checking and graph walking without deserializing all changeset metadata.

**VCS Mapping**
The `bonsai_git_mapping` and `bonsai_hg_mapping` facets provide bidirectional lookup between Bonsai changeset identifiers and Git or Mercurial commit hashes. These are used extensively when serving VCS clients.

Repository operations compose these facets to implement higher-level functionality like pushrebase, cross-repository sync, and hooks.

## Summary

The Bonsai data model serves as Mononoke's canonical representation of version control data. Its design reflects several architectural principles:

**VCS Independence** - Bonsai is not tied to Git or Mercurial implementation details, allowing it to serve as an intermediate representation.

**Content Addressing** - Blake2b hashing creates a Merkle DAG ensuring data integrity and enabling efficient deduplication.

**Minimal Representation** - Bonsai contains only the information necessary to represent commit history, with additional indexes provided by derived data.

**Flat File Changes** - The flat list of file changes simplifies many operations compared to nested tree structures.

**Immutability** - Once created, Bonsai changesets never change, enabling aggressive caching and simplifying consistency reasoning.

**Explicit Semantics** - Copy-from information, file types, and metadata are explicitly represented rather than inferred.

## Related Documentation

- [Key Concepts](1.2-key-concepts.md) - Brief introduction to Bonsai and other core concepts
- [Architecture Overview](1.3-architecture-overview.md) - How Bonsai fits into the overall system
- [Derived Data](2.3-derived-data.md) - Data structures computed from Bonsai changesets
- [Repository Facets](2.2-repository-facets.md) - How to access Bonsai data through facets
- [Storage Architecture](2.4-storage-architecture.md) - How Bonsai changesets are stored
