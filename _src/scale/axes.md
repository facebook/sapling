---
sidebar_position: 20
---
# Axes of scale

Source control clients face many different kinds of scaling challenges.  This document aims to list those various challenges, and provide an ultra-concise explanation of how we tackle it.  Links to deeper documents will cover individual areas in more depth.

Two notes before reading:

1. The problems below are specific to source control clients, that is, the piece that you run on your local computer.  The source control server faces similar scaling challenges and unique solutions, but those are not covered here.
2. Some of the solutions described below require the Sapling client to work with the Sapling server.  When using the Sapling client with a Git server, some of these optimizations do not apply.

## Working copy scale

The working copy consists of all the files you have checked out and interact with.  This is all the files in your repository directory, except the ones in the .sl directory.

### Number of files

Having millions of files in the working copy causes numerous problems for traditional version control. Checkouts are slow because we must download and write all of those files to disk.  Status is slow because we must scan every file in the working copy to determine what has changed.

Sapling tackles large numbers of files in three ways:

* The Watchman filesystem monitor lets Sapling monitor what files have changed, allowing it to answer status queries in O(files-you-have-changed) time instead of scanning the repository.
* The sparse feature allows users to only checkout the files that are relevant to them.
* The Sapling compatible virtual filesystem makes it appear as though you have the entire repository, but files are only downloaded when you first access them.

### Size of files

Moderately large files (10MB-2GB) are not a major source of scaling problems, but still require special consideration.  Large files are downloaded from a special server that speaks the Git LFS protocol to avoid overloading the primary source control service.  Large files are also stored locally in their own portion of the data cache, so that they have their own cache size limit and don’t swamp the other kinds of data.

Files larger than 2GB are currently not battle-tested in Sapling, though they may still work.

### Size of directories

Large directories are those with thousands or tens of thousands of immediate children files or directories.  It does not refer to directories which recursively contains many files or directories.

Directories up to a few thousand children are reasonably well supported in Sapling. Beyond that performance starts to degrade.  Further optimizations are possible, but this is not an issue on the current Sapling-based monorepos and thus far not a priority.

## Repository scale

The repository consists of all the behind-the-scenes, non-working-copy repository data that is stored in the .sl directory. Similar to the .git directory in Git.

### Number of commits

The total number of commits in a repository constantly grows over time, and in a large organization can easily reach millions or tens of millions of commits.  This affects a wide variety of performance characteristics:

* Storing all the commit data (messages, author, metadata, etc.) can take gigabytes of space. Note, this does not include the cost of storing file and tree data.
* Running `sl log` with particular constraints may have to read a large number of commits.
* Common graph queries, like ‘common ancestor’, used in many commands, like `smartlog` and `rebase`, can become slow.
* Computing the shortest unique hash prefix to provide pleasing UI output can become expensive.

Sapling takes several approaches to handle this:

* History data (i.e. the graph relationships) and commit metadata (i.e. messages, authors, etc) are stored separately. This allows us to download lots of history data for graph computations, while downloading a more limited amount of commit metadata.
* Commit metadata is downloaded lazily. When you first clone a repository, no commit messages are downloaded. Only once you start inspecting commits, via `smartlog`, `show`, etc, individual commit metadata is downloaded as needed. This makes clones fast, and reduces the amount of disk space required to O(commits you are interested in).
* History data is stored using the Segmented Changelog. This is a data structure that represents the shape of the entire commit graph using segments to concisely represent large swaths of history.  The segmented changelog can be represented in just a few megabytes and can answer graph query questions in O(number-of-merges) time, or just a few milliseconds even in a large monorepo.
* `log` with certain constraints, such as trying to find all commits by a certain user, may still need to inspect a lot of commits.  Sapling attempts to make `log` more efficient by batch-fetching commit data, in order to avoid one-by-one serial fetches.  Additionally, `log` on files and directories has special support, discussed below.
* Computing the shortest unique hash for a given commit is done by maintaining an incrementally updatable tree-like index of all locally-available commit hashes.

### Number of branches/bookmarks

As the number of remote bookmarks (similar to a Git remote branch) in a repository grows over time, this can result in large number of remote bookmarks being downloaded and monitored by the client. This can cause slowness when talking to the server, as each bookmark needs to be checked, and generally clutters the local user experience.

Sapling defaults to not downloading every bookmark from the server. Instead, during a clone you receive the `main` or `master` bookmark, and a configurable list of specific other bookmarks.  Users can choose to subscribe to specific bookmarks via `sl pull -B`.  Sapling also transparently downloads bookmarks if the users uses them in commands. For instance, if a user runs `sl goto release_123` but they don’t have that remote bookmark, Sapling will automatically download it to complete the checkout.

### Quantity of historical file/tree data

During a normal distributed source control pull or clone you download all the new files and trees that have been changed, resulting in your local client having all of history. In a large repository, this quantity of data may be so large that it is slow and impractical to download all of it to individual clients.

Sapling, on the other hand, does not downloald tree and file data durings pull and clones. That data is left on the server, and the Sapling client downloads it on demand when later required, such as during a checkout.  The on demand nature of Sapling commands requires carefully designed algorithms to ensure data is fetched efficiently and in parallel.

Lazily downloading data means Sapling may need network access to perform operations that would traditionally be doable offline.  To support some offline work, Sapling keeps all of the downloaded data in a local cache with a bounded size.  This cache generally contains enough data to move between, inspect, commit, and amend on any of your recently in-progress commits.


### Length of file history

In many version control systems, running commands like `log` and `blame` on a file or directory often have to walk every commit in the repository looking for commits that touched that file or directory. This is an O(size of repo) operation.

For files, Sapling makes these operations O(changes to that file) by tracking the exact history of each individual file, in addition to the history of the entire commit graph.  This allows log and blame to be fast, regardless of the size of the repository.

For directories, Sapling also tracks the history of a directory, but using this information to answer history queries is not yet implemented. Instead, Sapling can query the Sapling server for a directory’s history, which allows using the server's superior indexes to answer the query quickly.

Additionally, in the case where the server isn't available, Sapling can bisect over the Segmented Changelog structure to look for the commits that changed the given file or directory. This allows figuring out an approximate history in O(log n) time, though it may miss cases where a file or directory is changed, then reverted back to a previous version.

### Number of commits per hour

In a large organization the number of commits being pushed per minute introduces additional scaling challenges.

**Rebase races**

In Git or Mercurial, in order to push to the `main` branch you must first pull and rebase/merge onto the latest main branch.  If someone pushes before you, then you have to repeat the process until you win. If there are many people competing, it can become almost impossible to actually push your commit.

In Sapling, when you push to the Sapling server, the server actually takes your commit and rebases it to be on the top of bookmark you are pushing to.  So if someone else pushed before you, it’s ok because the server just moves your commit up to the top.  If someone else edited a file you touched, then the push will fail and you must manually rebase to merge the file.  There is still the potential for races where two people modify two different files in incompatible ways, but in practice this has not been an issue.

**Code generation races**

A large amount of commit throughput also introduces problems if your repository contains generated files.  If you modify a file that requires regenerating, then if someone else does the same and pushes first, you need to rebase over their change and once again regenerate the files.  The time taken to generate the file means the window in which you could lose the race becomes quite large, and it can become impossible to win the race if many people are changing the same generated files.

While Sapling doesn’t completely solve this, it improves the user experience by supporting calling code generators to solve rebase conflicts.  This allows conflicts in generated code to be handled at rebase time and allows code-pushing automation to automatically regenerate code on the users behalf when it encounters a push failure due to a conflict.
