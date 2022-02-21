# Mononoke

Mononoke is a next-generation server for the [Mercurial source control
system](https://www.mercurial-scm.org/), meant to scale up to accepting
thousands of commits every hour across millions of files. It is primarily
written in the [Rust programming language](https://www.rust-lang.org/en-US/).

## Caveat Emptor

Mononoke is still in development. We are making it available now because we plan to
start making references to it from our other open source projects.

**The version that we provide on GitHub is omitting some functions**.

This is because the code is exported verbatim from an internal repository at Facebook, and
not all of the scaffolding from our internal repository can be easily extracted. The key areas
where we need to shore things up are:

* Support for running thrift based apis.
* Production metadata SQL support (e.g. something like a MySQL backend).  We provide sqlite in OSS for now.
* Production blobstore storage backends (e.g. something like S3).  We provide SQL (on sqlite) and File System backends currently in OSS.

Linux is Mononoke's primary target plaform with OSS CI also running on MacOS. Other Unix-like OSes may be supported in the future.

## Subsystem Docs

Most of our documentation is in internal systems, however a few subsystems have in-repo markdown docs available:

* [Integration Tests](tests/integration/README.md)
* [Packblob Storage](blobstore/packblob/README.md) how Mononoke compressed store works
* [Graph Walker](walker/src/README.md) used to check/scrub storage
