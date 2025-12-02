# Derived Data

This document explains Mononoke's derived data system—the framework for computing and caching indexes and alternative representations from the canonical Bonsai changesets and file content blobs.

## What is Derived Data?

Derived data is information computed from Bonsai changesets and file contents that can always be regenerated from the core data. While Bonsai changesets and file content blobs constitute the source of truth for repository history, many operations would be inefficient using only this minimal representation. Derived data provides indexes, directory structures, file metadata, and VCS-specific formats that enable performant read operations.

Examples of derived data include:
- Directory manifests that allow traversing file hierarchies
- File history information for blame and log operations
- Git trees and commits for serving Git clients
- Mercurial filenodes and manifests for Mercurial protocol compatibility
- Case conflict detection data
- Deleted file tracking

The distinguishing characteristic of derived data is that it can be computed asynchronously after a commit has been accepted, rather than requiring synchronous computation during the commit operation.

## Separation from Core Data

Mononoke separates repository data into two categories:

**Core Data** - Information that must be written synchronously during commit operations:
- Bonsai changesets (parents, author, message, file changes)
- File content blobs
- VCS mappings (Bonsai ↔ Git/Mercurial identifiers)
- Commit graph index (parent-child relationships)

**Derived Data** - Information computed from core data after the commit completes:
- Directory manifests (various types)
- File history and blame
- VCS-specific formats (Git trees, Mercurial manifests)
- Repository analysis data

This separation allows Mononoke to maintain high commit throughput. When a commit is pushed, only core data is written before the operation completes and the next commit can proceed. Derived data is computed asynchronously by background workers or on-demand when needed for read operations.

### Write Path and Read Path

The write path accepts commits and stores core data:

1. Validate incoming commit from client (Git or Sapling)
2. Convert to Bonsai format
3. Store Bonsai changeset and file contents in blobstore
4. Update VCS mapping in metadata database
5. Add to commit graph
6. Run hooks and update bookmark if being pushed to a public bookmark
8. Push completes

Derived data computation is not on this critical path. The next commit can proceed immediately.

The read path uses derived data to serve queries efficiently:

1. Client requests data (file content, directory listing, blame)
2. Server determines which derived data type is needed
3. Check if derived data exists for the requested changeset
4. If not derived, compute it
5. Return result to client

For repositories with high commit rates, this separation prevents derived data computation from becoming a bottleneck. Additional derived data types can be added without affecting write latency.

## The Derived Data Framework

Derived data types are implemented using a trait-based framework centered on the `BonsaiDerivable` trait. This framework handles dependency management, caching, batch derivation, and storage.

### BonsaiDerivable Trait

Each derived data type implements `BonsaiDerivable`, which defines:

**Core Methods:**
- `derive_single` - Compute derived data for one changeset given its Bonsai representation and the derived data for its parents
- `store_mapping` - Store the derived data in a way that can be retrieved by changeset ID
- `fetch` - Retrieve previously computed derived data by changeset ID

**Optimization Methods:**
- `derive_batch` - Compute derived data for multiple changesets efficiently (default: sequential derivation)
- `fetch_batch` - Retrieve derived data for multiple changesets efficiently (default: individual fetches)

**Dependencies:**
- `Dependencies` type - Other derived data types that must be available before this type can be derived
- `PredecessorDependencies` type - Optional predecessor types that can be used for parallel backfilling

The framework is located in `derived_data/` and `derived_data/manager/`.

### Dependency Graph

Derived data types can depend on other derived data types. These dependencies form a directed acyclic graph that the derivation framework respects. When deriving a changeset, all dependencies are derived first.

Example dependencies:
- Blame depends on Unodes (file history manifests)
- Basename Suffix Skeleton Manifest V3 depends on Skeleton Manifest V2
- Git Delta Manifest V2 depends on Git Commit
- Mercurial Augmented Manifest depends on Mercurial Changesets

Dependencies are declared using the `Dependencies` associated type in the `BonsaiDerivable` implementation. The framework automatically ensures dependencies are satisfied before derivation proceeds.

### Storage

Derived data is stored in the blobstore using content-addressed keys. Each derived data type defines its own key format, typically incorporating the changeset ID and a type-specific prefix.

Example key patterns:
- `derived_root_blame_v2.<ChangesetId>` - Blame data
- `derived_root_fsnode.<ChangesetId>` - Fsnode manifest root

Mappings from changeset IDs to derived data are also stored in the metadata database for some types, providing fast lookups without requiring blobstore access.

## Derivation Processes

Derived data can be computed in several ways depending on operational needs.

### On-Demand Derivation

When a client requests data that requires a specific derived data type, and that data has not yet been derived for the requested changeset, derivation occurs on-demand. The server derives the data, stores it, and returns the result.

On-demand derivation follows the dependency graph: if type A depends on type B, and neither is derived, type B is derived first, then type A.

### Batch Derivation

For bulk operations or backfilling, derived data can be computed in batches. The `derive_batch` method allows implementations to optimize derivation across multiple changesets by:
- Batching database queries
- Amortizing common computations
- Parallelizing independent derivations

Batch derivation is used by the derived data service and bulk derivation tools.

### Backfilling

Backfilling is the process of deriving data for all changesets in a repository, typically after introducing a new derived data type or upgrading to a new version. Backfilling can be performed:

**Sequentially** - Derive changesets in topological order from repository roots to heads. Each changeset has its parents already derived.

**In Parallel with Slicing** - Divide the commit graph into slices, derive data for slice boundaries using predecessor dependencies, then derive each slice in parallel.

**Using Predecessor Optimization** - Some types support `derive_from_predecessor`, which can compute derived data using a different derived data type without requiring parent data. This enables parallel backfilling by deriving "anchor points" first.

Backfilling is performed using the admin CLI or bulk derivation tools in `derived_data/bulk_derivation/`.

### Incremental Derivation

After backfilling or initial derivation, new commits are derived incrementally as they arrive. This is typically handled by:
- On-demand derivation when data is requested
- Background workers deriving data for recent commits
- The derived data service coordinating derivation across workers

The warm bookmark cache mechanism ensures that derived data for public bookmarks (like `master`) is kept up-to-date.

## Remote Derivation Service

The Remote Derivation Service (`derived_data/remote/`) provides centralized, asynchronous derivation. Instead of each Mononoke server deriving data locally, derivation requests are enqueued and processed by a pool of derivation workers.

**Architecture:**
- **Service** - Thrift API server that accepts derivation requests and enqueues them
- **Workers** - Background processes that poll the queue and perform derivation
- **Queue** - Dependency-aware queue that ensures derived data dependencies are satisfied (implemented in `repo_attributes/repo_derivation_queues/`)

**API:**
- `derive` - Request asynchronous derivation, returns a token
- `poll` - Check status of a derivation request using the token

**Benefits:**
- Deduplication of derivation work (multiple requests for the same changeset coalesce)
- Horizontal scaling of derivation capacity by adding workers
- Reduced memory pressure on frontend servers
- Centralized control and monitoring of derivation

The service is defined in `derived_data/remote/if/derived_data_service.thrift` and implemented in `facebook/derived_data_service/`.

## Derived Data Types

Mononoke includes a variety of derived data types serving different purposes. These are organized into several categories.

### Manifest Types

Manifests represent directory structures in different formats.  Manifests can be tree-based or flat, and can also be sharded or unsharded.  Unsharded manifests suffer from performance issues as the number of files in a directory increases, while sharded manifests can scale to large directories.

**Fsnodes** (`derived_data/fsnodes/`)
- File and directory manifest using a tree structure
- Stores file metadata (size, content hash, file type)
- Used for directory traversal and file lookups
- Not sharded

**Unodes** (`derived_data/unodes/`)
- Manifest with file history linkage
- Tracks file and directory parentage for history traversal
- Allows following file history without traversing all commits
- Not sharded

**Skeleton Manifest** (`derived_data/skeleton_manifest/`)
- Directory structure without file contents
- Used for path-based queries
- Supports case conflict detection (legacy)
- Not sharded

**Skeleton Manifest V2** (`derived_data/skeleton_manifest_v2/`)
- Updated skeleton manifest using Sharded Map V2
- Sharded
- Does not support case conflict handling (moved to separate type)

**Basename Suffix Skeleton Manifest V3** (`derived_data/basename_suffix_skeleton_manifest_v3/`)
- Indexed by file basename suffix and path prefix
- Enables efficient file search operations of the form `prefix/**/*.suffix`.
- Depends on Skeleton Manifest V2
- Uses Sharded Map V2 for improved performance with large directories
- Flat and sharded

**Case Conflict Skeleton Manifest** (`derived_data/case_conflict_skeleton_manifest/`)
- Tracks case conflicts in paths
- Separated from Skeleton Manifest V2
- Flat and sharded

**Content Manifest** (`derived_data/content_manifest_derivation/`)
- Alternative to Fsnodes using Sharded Map V2, still in development
- Content-addressed manifest representation
- Will also support Git LFS pointers in the future
- Sharded

**Deleted Manifest V2** (`derived_data/deleted_manifest/`)
- Tracks deleted files and directories
- Enables efficient "was this path ever deleted?" queries
- Sharded using the legacy Sharded Map V1

### File Metadata Types

**Filenodes** (`derived_data/filenodes_derivation/`)
- Mercurial-compatible file history
- Maps file paths to their history in Mercurial format
- Required for Mercurial protocol compatibility

**Fastlog** (`derived_data/fastlog/`)
- Efficient file history lookup
- Pre-computed file history to avoid graph traversal

**Blame V2** (`derived_data/blame/`)
- Line-by-line attribution of file contents
- Depends on Unodes for file history

**Inferred Copy From** (`derived_data/inferred_copy_from/`)
- Copy/move detection using heuristics
- Similar to Git's copy detection

### Git-Specific Types

**Git Commit** (`git/git_types/`)
- Git commit objects derived from Bonsai
- Includes Git trees representing directory structure
- Enables serving Git protocol

**Git Delta Manifest V2** (`git/git_types/`)
- Optimized representation for Git packfile generation
- Depends on Git Commit
- Uses Sharded Map V2

**Git Delta Manifest V3** (`git/git_types/`)
- Next generation delta manifest
- Further optimizations for packfile generation

### Mercurial-Specific Types

**Mercurial Changeset** (`derived_data/mercurial_derivation/`)
- Mercurial changeset derived from Bonsai
- Required for Mercurial protocol compatibility
- Includes Mercurial manifests representing directory structure

**Mercurial Augmented Manifest** (`derived_data/mercurial_derivation/`)
- Content-addressed Mercurial manifest
- Alternative to traditional Mercurial manifests necessary for integration with CASC
- Includes additional metadata

### Utility Types

**Changeset Info** (`derived_data/changeset_info/`)
- Cached changeset metadata (author, message, parents)
- Enables fast metadata queries without deserializing full Bonsai changeset

**Test Manifest** (`derived_data/test_manifest/`)
- Manifest type for testing the derived data framework
- Not used in production

**Test Sharded Manifest** (`derived_data/test_sharded_manifest/`)
- Sharded manifest for testing
- Validates Sharded Map implementation

## Sharded Maps

Several manifest types use Sharded Maps, a specialized data structure for storing large mappings across multiple blobstore blobs. Sharded Maps allow loading subsets of a manifest without fetching the entire structure.

**Sharded Map V2** - Improved implementation with:
- Trie-based structure with path compression
- Terminal nodes for small subtrees (stored inline)
- Bounded node sizes even for very large directories
- Efficient partial loading for path lookups

Sharded Maps are used by Skeleton Manifest V2, Basename Suffix Skeleton Manifest V3, Content Manifest, and Git Delta Manifest V2.

## Versioning and Migration

Derived data types can be versioned, allowing the data model to evolve while maintaining backward compatibility.

**Versioning Strategy:**
- New versions are implemented as separate derived data types (e.g., Blame → Blame V2)
- Old versions continue to exist until no longer needed
- Repositories can be migrated by backfilling the new version
- Configuration controls which version is active per repository

**Adding a New Version:**
1. Implement new type with `BonsaiDerivable`
2. Define storage format and keys
3. Add to repository configuration
4. Backfill for existing changesets
5. Update clients to use new version
6. Deprecate and remove old version when migration is complete

This allows derived data improvements without downtime or risky migrations.

Existing types can be rederived using a mapping key prefix, defined in `DerivedDataTypesConfig::mapping_key_prefixes`.  This prefix is added to all mapping keys before the changeset id, and separates them into a different namespace so that rederivation can be performed independently.

## Adding New Derived Data Types

To add a new derived data type:

1. **Define the Type** - Create a struct representing the derived data
2. **Implement BonsaiDerivable** - Define how to derive from Bonsai and parents
3. **Declare Dependencies** - Specify any required derived data types
4. **Define Storage** - Implement `store_mapping` and `fetch` methods
5. **Register Type** - Add to the `DerivableType` enum in `mononoke_types`
6. **Configure** - Add to repository configuration to enable
7. **Test** - Write unit and integration tests
8. **Backfill** - Derive for existing changesets

**Example locations:**
- Derived data implementations: `derived_data/*/`
- Framework: `derived_data/manager/`
- Type registration: `mononoke_types/src/derivable_type.rs`
- Configuration: `metaconfig/types/`

The `BonsaiDerivable` trait documentation in `derived_data/src/lib.rs` provides detailed usage information.

## Performance Considerations

The derived data system is designed for performance at scale:

**Batch Derivation** - Deriving multiple changesets together reduces database round-trips and allows amortization of common work.

**Dependency Management** - The framework ensures dependencies are derived in the correct order, minimizing redundant derivation.

**Remote Derivation** - Centralizing derivation reduces duplicate work across servers and enables horizontal scaling.

**Caching** - Derived data is stored in the blobstore and benefits from multi-level caching (cachelib, memcache).

**Incremental Computation** - Most derived data types can compute new values based on parent values, avoiding full recomputation.

**Selective Derivation** - Not all derived data types need to be derived for all changesets. Configuration controls which types are active.

## Related Documentation

* [Architecture Overview](1.3-architecture-overview.md) - System context and write/read path separation
* [Key Concepts](1.2-key-concepts.md) - Brief derived data introduction
* [Bonsai Data Model](2.1-bonsai-data-model.md) - Core data that derived data builds upon
* [Repository Facets](2.2-repository-facets.md) - RepoDerivedData facet
* [Storage Architecture](2.4-storage-architecture.md) - How derived data is stored

Component-specific documentation for individual derived data types lives in the respective directories under `derived_data/`.
