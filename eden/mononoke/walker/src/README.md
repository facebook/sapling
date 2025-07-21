# Mononoke Graph Walker

## Overview
The Mononoke Graph Walker (henceforth just the walker) defines a graph schema for mononoke across both its SQL and Blobstore stored types, and uses it to provide the following functionality.

- scrubbing of underling blobstores to ensure durability
- validation of data in the underlying storage to detect logic errors (e.g. dangling references)

In the future it is intended to provide other operations over the mononoke graph, including
  - corpus collection
    - for compression analysis
    - possibly for backup (in situations where full repo too large)
  - blob compression
    - e.g. group blobs by type/repopath and then compress with shared dictionary or zstd deltas
  - soft gc/archival of data by comparing the graph walk visited maps vs a blobstore enumeration
  - further validation
    - e.g. hash validation

## Graph

The walker represents Mononoke as a graph schema of `Node`s connected by edges.  The `Node`'s are kept small to just their identities, with `NodeData` representing the payload.  A node also has a `NodeType`, typically used for filtering out a class of nodes.

The graph is dynamically discovered, so edges are represented as an `OutgoingEdge` pointing to a target and annotated by an `EdgeType` and optionally a `WrappedPath` (which represents a path in the repo).  There is currently no type representing a full edge,  if that is needed then a `Node` plus an `OutgoingEdge` would suffice.

The types are represented as Enum variants so they can be passed in heterogeneous collections, in particular the queues used by `bounded_traversal_stream` which does the dynamic unfolding of the graph.

Note that due to backlinks from file to commit graph (e.g. `HgFileNodeToHgChangeset`)  the graph has cycles so tracking visits is important to be able to break cycles.

A walk is said to be deep if it will cover the entire history of the repo,  or shallow if it will only look at data (e.g. FileContent) accessible at a given commit.

### Howto add new walk steps

When extending the walker to cover step to a new type of `Node`,  one needs to:

- Add enum variants to `Node`, `NodeType`, `NodeData` and to `EdgeType` to describe the transitions.
  - `Node` should hold the minimal key needed to lookup this node (e.g. hash or hash + repopath)
  - `NodeData` should hold the data loaded by the Node that is used to derive the children.  The key thing is that it should be Some() if the load succeeded.  NodeData is currently populated for all Node's.
  - If if there is a both a mapping (e.g. Bonsai to Hg) and data, those are represented as two separate `Node`'s
- Add the new types to match statements,  the compiler will find these for you.
- Add new `_step()` functions in walk.rs that expand the `Node` to its children.
  - If reading your node may cause writes (e.g. data derivation),  make sure to honor the enable_derive flag, as in prod usage the walker runs with it set to false.  This may mean changes to the underlying code to support non-writing mode.
- Update existing `_step()` that are sources of edges to your `Node` to produce `OutgoingEdge` with your `Node` listed.
- If the node will be potentially re-visited, add visit tracking for it in `WalkStateCHashmap`
- Run the tests, and update test expectations to include your new `Node`
- Run for a real small repo without (baseline) and with your changes and make sure the results are as you expect.

### Multiple valid routes

There are often multiple valid routes from A to B, and because the graph is dynamically unfolded and evaluated in parallel from async IO, which route is chosen can vary between runs.  One way this is visible is in the number of checks for re-visits done per `NodeType`. If two nodes A and B expand to content X then at most one will visit it, but both will record a check.

### Mutable data

Most Mononoke data represented on the graph is immutable, so visit tracking can prevent any re-visits at all.

For bookmarks the solution is that published bookmark data is resolved once per walk and then referred to during the walk to ensure it is consistent within a given tailing run.

However for a subset of data, currently `BonsaiHgMapping` and `BonsaiPhaseMapping`,  the graph sees the data in more than one state.  In this case the data is too large to efficiently reload in a snapshot approach on each tail iteration,  so instead the `WalkStateCHashmap` state tracking will allow revisits until the `Node` has received its `NodeData` in the expected terminal state.

## WalkVisitor

The `WalkVisitor` trait is called at the start and end of the unfolding of a graph node,   with `start_node()` giving it a chance to setup CoreContext for sampling blobstore actions ( and maybe in the future for sampling SQL actions) before the walk attempts to load the target node, and `visit()` having the bulk of functionality where it sees the unfolded outgoing edges and has a chance to filter them to remove re-visits (e.g. `WalkStateCHashmap`) and to validate them e.g. (`ValidatingVisitor`)

## Memory Usage

Memory usage by the graph representation is one of the key design constraints of the walker, driven by two concerns:

- to avoid the dynamic graph revisiting the same node again and again, we must keep track of visited nodes. This is done via concurrent maps (currently CHashMap), see state.rs
- as the graph expands at each step, the queue of pending nodes to visit can get very large (100s of millions of entries)
  - the size of the Node representation is a big driver of this
  - for walks that maintain route information (e.g. previous Node visited), the Route is also a major concern for memory usage

There are several possible further memory usage improvements to consider.

- Interning `WrappedPath`, and `Node`
   - If paths still a major memory user even after interning,  intern MPathElement and/or intern to a prefix tree
- Only returning the `NodeData` objects required. Currently we return `NodeData` unconditionally, however these usually don't dominate, except for very large manifests (large files return a stream which is conditionally consumed).
- Return interator/stream of expanded nodes to avoid intermediate `Vec`'s from `collect()`
- Extending `bounded_traversal_stream` to reduced the size of pending queues as much as possible based on expected unfold size of a `Node`'s `NodeType`.
- Extend the sampling approach (see below)

## Sampling

To handle large repos,  the walker supports sampling by node hash and repo path so that actions can be run on a subset of the data.  Sampling may be based purely on Node identity (e.g. scrub), or on the route through the graph taken (e.g. `WrappedPath` tracking used in compression-benefit)

This is combined with the `SamplingBlobstore` to support sampling low level blobstore actions/data.  The `SamplingWalkVisitor` connects up the high level Node's being sampled with the lower level blobstore sampling via the `CoreContext`'s `SamplingKey`.

Combined with the in review `--sampling-offset` functionality,  sampling with `--sample-rate` provides a way to operated on a slice of a repo e.g. 1/100th or 1/1000th of it at a time,  and by incrementing the offset the entire repo can be covered a slice at a time. When sampling by `Node` hash this is stable, when sampling by repo path the are potentially multiple paths for a `FileContent`, so the slice assigned will be dependent on walk order.

Currently sampling is used only to restrict the output stage, e.g. which objects are attempted to be compressed or dumped to disk.  It could also be used to restrict the walk, e.g. into batches of commits.  Likely we'd still keep the `WalkStateCHashmap` or its equivalent populated between slices to avoid re-visits.

## Logging and Metrics

The walker's main production monitoring is via ODS metrics with scuba sampling of `Node`'s with problems.  Scuba logs can also optionally include route information, e.g. validate reports the source `Node` that a step originated from.

When running locally or in integration tests glog progress logging provides the stats, with scuba optionally logging to file or dropped.

# Subcommands

## Corpus

This walker can dump out a corpus of blobs from a repo via the `corpus` subcommand.  This is intended to allow shell level investigation and experimentation with the corpus,  e.g. trying various compression techniques.

To form a corps it associates an in-repo `WrappedPath` with `Node`s that either have a path as part of their identity (e.g. `Node::HgManifest`),  can be associated with a path as part of an edge,  (e.g. `OutgoingEdge` for `EdgeType::BonsaiChangesetToFileContent`), or can have a path associated based on the route taken through the graph (e.g. `Node::FileContentMetadata`).

The output directory is specified with --output-dir, and can be sampled by path-hash or path-regex

The blobs are dumped out in the output dir in the layout `NodeType`/<repo_path>/.mononoke,/aa/bb/cc/blob_key  where the repo_path and blob_key are both percent_encoded and the aa/bb/cc etc are a subset of the hash used to prevent any one directory becoming too large.  For types without any in-repo path (e.g. `BonsaiChangeset`) the repo_path component is omitted.  The repo paths and key are percent encoded so that they can't match the magic `.mononoke,` directory name as it contains a comma, and comma is percent encoded. This property can be used in shell operations to separate the blob structure from the in-repo path.

e.g to find all blobs and list them
```
$ find . -type d -name .mononoke,| xargs -i find {} -type f | head -1
./FileContent/root/foo/bar/baz/space_usage.json/.mononoke,/49/f0/c1/repo2103.content.blake2.49f0c1d254974a7eb43ad25e93426e738aa27e3f5e391301a1402e26643542c6
```

vs to find all files we have dumped blobs for
```
$  find . -type d -name .mononoke, |xargs dirname| head -1
./FileContent/root/foo/bar/baz/space_usage.json
```

## Scrub

The walker can check and optional repair storage durability via the `scrub` subcommand.  This checks each component of a multiplexed blobstore has data for each key, so that we could run on one side of the multiplex if necessary

Example issues this could correct:

  - One component blobstore has temporarily inaccessible subset of keys
  - Keys stored in only one component store
  - Invalid keys present in a store that are not loadable

Scrub makes sure that the above are detectable, and in the event of a component store failure we can run on one component store.

The scrub visits all graph nodes, with the underlying ScrubBlobstore providing a call back used when issues are detected.

## Validate

The walker can check data validity via the `validate` subcommand

Example issues this could detect:

  - Detect if linknodes have been missing and/or invalid
  - Detect public commits incorrectly labelled as non-public

## Compression Benefit/Sizing

This provides a tool to measure effective compression ratio to a repo if we were to zstd compress each blob individually via the `compression-benefit` subcommand.
