# Sapling SCM

Sapling SCM is a cross-platform, highly scalable, Git-compatible source control system.

It aims to provide both user-friendly and powerful interfaces for users, as
well as extreme scalability to deal with repositories containing many millions
of files and many millions of commits.

# Using Sapling
To start using Sapling, see the [Getting Started](https://sapling-scm.com/docs/introduction/getting-started) page for how to clone your existing Git repositories. Checkout the [Overview](https://sapling-scm.com/docs/overview/intro) for a peek at the various features. Coming from Git? Checkout the [Git Cheat Sheet](http://sapling-scm.com/docs/introduction/git-cheat-sheet).

Sapling also comes with an [Interactive Smartlog (ISL)](http://sapling-scm.com/docs/addons/isl) web UI for seeing and interacting with your repository, as well as a VS Code integrated Interactive Smartlog.

# The Sapling Ecosystem

Sapling SCM is comprised of three main components:

* The Sapling client: The client-side `sl` command line and web interface for users to interact
  with Sapling SCM.
* Mononoke: A highly scalable distributed source control server. (Not yet
  supported publicly.)
* EdenFS: A virtual filesystem for efficiently checking out large repositories.
  (Not yet supported publicly.)

Sapling SCM's scalability goals are to ensure that all source control operations
scale with the number of files in use by a developer, and not with the size of
the repository itself.  This enables fast, performant developer experiences even
in massive repositories with millions of files and extremely long commit histories.

### Sapling CLI

The Sapling CLI, `sl`, was originally based on
[Mercurial](https://www.mercurial-scm.org/), and shares various aspects of the UI
and features of Mercurial.

The CLI code can be found in the `eden/scm` subdirectory.

### Mononoke

[Mononoke](eden/mononoke/README.md) is the server-side component of Sapling SCM.

While it is used in production within Meta, it currently does not build in an
open source context and is not yet supported for external usage.

### EdenFS

EdenFS is a virtual file system for managing Sapling checkouts.

While it is used in production within Meta, it currently does not build in an
open source context and is not yet supported for external usage.

EdenFS speeds up operations in large repositories by only populating working
directory files on demand, as they are accessed.  This makes operations like
`checkout` much faster, in exchange for a small performance hit when first
accessing new files.  This is quite beneficial in large repositories where
developers often only work with a small subset of the repository at a time.

More detailed EdenFS design documentation can be found at
[eden/fs/docs/Overview.md](eden/fs/docs/Overview.md).

## Building the Sapling CLI

The Sapling CLI currently builds and runs on Linux, Mac, and Windows. It can be
built by running `make oss` in the `eden/scm` directory and running the
resulting `sl` executable.

Building the Sapling CLI requires Python 3.8, Rust, cmake, and OpenSSL for the main cli, and
Node and Yarn for the ISL web UI.

# License

See [LICENSE](LICENSE).
