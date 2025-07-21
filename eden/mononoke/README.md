# Mononoke

Mononoke is the server built for the [Sapling Source Control
System](https://sapling-scm.com/), meant to scale up to accepting thousands of
commits every hour across millions of files. It is primarily written in the
[Rust programming language](https://www.rust-lang.org/en-US/).

The open source build includes mysql, sqlite, file and S3 backends.

## Caveat Emptor

Mononoke is still in development. We are making it available now because we plan to
start making references to it from our other open source projects.

**The version that we provide on GitHub omits some functions**.

This is because the code is exported verbatim from an internal repository at Facebook, and
not all of the scaffolding from our internal repository can be easily extracted. The key areas
omitted are:

* Support for running Thrift based APIs.
* MySQL failover support.  You'll need to restart mononoke processes if the MySQL endpoint changes.
* CacheLib support. There is caching, but not yet using the [OSS CacheLib release](https://github.com/facebook/cachelib).
* Documentation on how to configure.  You can probably work out some hints from the tests.

Check GitHub Actions for the latest build/test status. Linux is Mononoke's primary target platform with OSS CI also running on MacOS. Other Unix-like OSes may be supported in the future.

## Subsystem Docs

Most of our documentation is in internal systems, however a few subsystems have in-repo markdown docs available:

* [Integration Tests](tests/integration/README.md)
* [Packblob Storage](blobstore/packblob/README.md) how Mononoke compressed store works
* [Graph Walker](walker/src/README.md) used to check/scrub storage
