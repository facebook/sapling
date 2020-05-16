# EdenSCM

EdenSCM is a cross-platform, highly scalable source control management system.

It aims to provide both user-friendly and powerful interfaces for users, as
well as extreme scalability to deal with repositories containing many millions
of files and many millions of commits.

EdenSCM is comprised of three main components:

* The `eden` CLI: The client-side command line interface for users to interact
  with EdenSCM.
* Mononoke: The server-side part of EdenSCM.
* EdenFS: A virtual filesystem for efficiently checking out large repositories.

EdenSCM's scalability goals are to ensure that all source control operations
scale with the number of files in use by a developer, and not with the size of
the repository itself.  This enables fast, performant developer experiences
even in massive repositories with many long files and very long commit
histories.


# The `eden` CLI

The `eden` CLI was originally based on
[Mercurial](https://www.mercurial-scm.org/), and shares many aspects of the UI
and features of Mercurial.

The CLI code can be found in the `eden/scm` subdirectory.

## Building the `eden` CLI

The `eden` CLI currently builds and runs on Linux, Mac, and Windows.  The
`setup.py` script is the main interface for building the CLI.


# Mononoke

Mononoke is the server-side component of EdenSCM.

Despite having originally evolved from Mercurial, EdenSCM is not a distributed
source control system.  In order to support massive repositories, not all
repository data is downloaded to the client system when checking out a
repository.  Clients ideally only download the minimal amount of data
necessary, and then fetch additional data from the server as it is needed.

## Building Mononoke

The Mononoke code lives under `eden/mononoke`

Mononoke currently builds and runs only on Linux, and is not yet buildable
outside of Facebook's internal environment.  Work is still in progress to
support building Mononoke with Rust's `cargo` build system.


# EdenFS

EdenFS is a virtual file system for managing EdenSCM checkouts.

EdenFS speeds up operations in large repositories by only populating working
directory files on demand, as they are accessed.  This makes operations like
`checkout` much faster, in exchange for a small performance hit when first
accessing new files.  This is quite beneficial in large repositories where
developers often only work with a small subset of the repository at a time.

EdenFS has similar performance advantages to using sparse checkouts, but a much
better user experience.  Unlike with sparse checkouts, EdenFS does not require
manually curating the list of files to check out, and users can transparently
access any file without needing to update the profile.

EdenFS also keeps track of which files have been modified, allowing very
efficient `status` queries that do not need to scan the working directory.
The filesystem monitoring tool [Watchman](https://facebook.github.io/watchman/)
also integrates with EdenFS, allowing it to more efficiently track updates to
the filesystem.

More detailed EdenFS design documentation can be found at
[eden/fs/docs/Overview.md](eden/fs/docs/Overview.md).

## Building EdenFS

EdenFS currently builds on Linux, Mac, and Windows.

The recommended way to build EdenFS is using the `build.sh` script in the
top-level of the repository.  This script will download and build all of the
necessary dependencies for EdenFS, before building EdenFS itself.  On Windows
use the `build.bat` script instead of `build.sh`.

This build script will create an output directory outside of the repository
where it will perform the build.  You can control this output directory
location by passing a  `--scratch-path` argument to the build script.

# Support

EdenSCM is the primary source control system used at Facebook, and is used for
Facebook's main [monorepo](https://en.wikipedia.org/wiki/Monorepo) code base.

Support for using EdenSCM outside of Facebook is still highly experimental.
While we would be interested to hear feedback if you run into issues,
supporting external users is not currently a high priority for the development
team, so we unfortunately cannot guarantee prompt support at this time.

# License

See [LICENSE](LICENSE).
