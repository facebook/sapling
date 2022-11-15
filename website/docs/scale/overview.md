---
sidebar_position: 10
---

# Overview

Sapling supports large monorepos that have tens of millions of files and
commits, with tens of thousands of contributors.

## Performance challenges

This scale imposes performance challenges in various areas. Operations that
require all files or all commits (O(files) or O(commits)) space or time
complexities are gradually no longer affordable. Push throughput could also be
an issue.

Over time, Sapling made many improvements to tackle the above challenges:

- On-demand historical file fetching (remotefilelog, 2013)
- File system monitor for faster working copy status (watchman, 2014)
- In-repo sparse profile to shrink working copy (2015)
- Limit references to exchange (selective pull, 2016)
- On-demand historical tree fetching (2017)
- Incremental updates to working copy state (treestate, 2017)
- New server infrastructure for push throughput and faster indexes (Mononoke, 2017)
- Virtualized working copy for on-demand currently checked out file or tree fetching (EdenFS, 2018)
- Faster commit graph algorithms (segmented changelog, 2020)
- On-demand commit fetching (2021)

## Other challenges

Besides improving scale and performance, we also strove to build a robust
development experience.  To avoid developers losing their work due to hardware
failures, we back up all commits as they are created to our "commit cloud".
Unlike other systems, the developer doesn't have to expressly push their
commits for them to be shareable and durable.

## Note about "Distributed"

Sapling started from Mercurial as a distributed source control system, it then
transitioned to a client-server architecture to solve the challenges.
The server can utilize distributed storage and pre-build various kinds of
indexes to provide more efficient operations.

While Sapling is less "distributed", we tried to make the difference
transparent to the user. For example, our lazy commit graph implementation does
not require extra commands to "deepen" the graph. The user sees full history.

That said, we do drop support of pulling from a lazy repo. Commit cloud covers
these use-cases.
