# EdenFS is a FUSE virtual filesystem for source control repositories.

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

# Building EdenFS

EdenFS currently only builds on Linux.
We have primarily tested building it on Ubuntu 18.04.

## TL;DR

```
[eden]$ ./getdeps.py --system-deps
[eden]$ mkdir _build && cd _build
[eden/_build]$ cmake ..
[eden/_build]$ make
```

## Dependencies

EdenFS depends on several other third-party projects.  Some of these are
commonly available as part of most Linux distributions, while others need to be
downloaded and built from GitHub.

The `getdeps.py` script can be used to help download and build EdenFS's
dependencies.

### Operating System Dependencies

Running `getdeps.py`  with `--system-deps` will make it install third-party
dependencies available from your operating system's package management system.
Without this argument it assumes you already have correct OS dependencies
installed, and it only updates and builds dependencies that must be compiled
from source.

### GitHub Dependencies

By default `getdeps.py` will check out third-party dependencies into the
`eden/external/` directory, then build and install them into
`eden/external/install/`

If repositories for some of the dependencies are already present in
`eden/external/` `getdeps.py` does not automatically fetch the latest upstream
changes from GitHub.  You can explicitly run `./getdeps.py --update` if you
want it to fetch the latest updates for each dependency and rebuild them from
scratch.
