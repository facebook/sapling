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
`setup.py` script is the main interface for building the CLI, however to prepare
and install all the dependencies its recommended to start off with a getdeps.py build
as per the [Build Notes](#Build_Notes)

# Mononoke

[Mononoke](eden/mononoke/README.md) is the server-side component of EdenSCM.

Despite having originally evolved from Mercurial, EdenSCM is not a distributed
source control system.  In order to support massive repositories, not all
repository data is downloaded to the client system when checking out a
repository.  Clients ideally only download the minimal amount of data
necessary, and then fetch additional data from the server as it is needed.

## Building Mononoke

The Mononoke code lives under `eden/mononoke`

Mononoke is built using Rust's `cargo` build system however to prepare and install all the dependencies 
its recommended to start off with a getdeps.py build as per the [Build Notes](#Build_Notes)

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

Eden is built using a combination of `cmake` and `cargo` with `cmake` as the 
main entry point, however to prepare and install all the dependencies 
its recommended to start off with a getdeps.py build as per the [Build Notes](#Build_Notes)

This build script will create an output directory outside of the repository
where it will perform the build.  You can control this output directory
location by passing a  `--scratch-path` argument to the build script.

On Ubuntu, either let getdeps install the requirements or read `requirements_ubuntu.txt`.
You will also need m4 and Rust installed.

# Support

EdenSCM is the primary source control system used at Facebook, and is used for
Facebook's main [monorepo](https://en.wikipedia.org/wiki/Monorepo) code base.

Support for using EdenSCM outside of Facebook is still highly experimental.
While we would be interested to hear feedback if you run into issues,
supporting external users is not currently a high priority for the development
team, so we unfortunately cannot guarantee prompt support at this time.

# License

See [LICENSE](LICENSE).

# Build Notes

## `getdeps.py`

This script is used by many of Meta's OSS tools.  It will download and build all of the necessary dependencies first, and will then invoke cmake etc to build the Eden components.  This will help ensure that you build with relevant versions of all of the dependent libraries, taking into account what versions are installed locally on your system.

It's written in python so you'll need python3.6 or later on your PATH.  It works on Linux, macOS and Windows.

The settings for eden's cmake builds are held in its getdeps manifests: Eden CLI: `build/fbcode_builder/manifests/eden_scm`, EdenFS:  `build/fbcode_builder/manifests/eden`, and Mononoke: `build/fbcode_builder/manifests/mononoke` which you can edit locally if desired.  Most getdeps commands take the manifest name as a parameter(example below).

### Dependencies

If on Linux or MacOS (with homebrew installed) you can install system dependencies to save building them:

    # Clone the repo
    git clone https://github.com/facebookexperimental/eden
    # Install dependencies
    cd eden
    sudo ./build/fbcode_builder/getdeps.py install-system-deps --recursive [eden_scm|eden|mononoke]

If you'd like to see the packages before installing them:

    ./build/fbcode_builder/getdeps.py install-system-deps --dry-run --recursive [eden_scm|eden|mononoke]

On other platforms or if on Linux and without system dependencies `getdeps.py` will mostly download and build them for you during the build step.

NB: `getdeps.py` won't install the C++ toolchain or Rust toolchain for you.

### Build

This script will download and build all of the necessary dependencies first,
and will then invoke cmake and cargo etc to build the components of EdenSCM.  
This will help ensure that you build with relevant versions of all of the dependent libraries,
taking into account what versions are installed locally on your system.

`getdeps.py` currently requires python 3.6+ to be on your path.

`getdeps.py` will invoke cmake and cargo etc for a manifest (here seen for `eden_scm`)

    # Clone the repo
    git clone https://github.com/facebookexperimental/eden
    cd eden
    # Build, using system dependencies if available
    python3 ./build/fbcode_builder/getdeps.py --allow-system-packages build eden_scm

Specify `eden_scm` for Eden CLI, `eden` for EdenFS, or `mononoke` for Mononoke build.

It puts output in its scratch area.  You can find the default scratch install location from logs or with `python3 ./build/fbcode_builder/getdeps.py show-inst-dir eden_scm`

You can also specify a `--scratch-path` argument to control
the location of the scratch directory used for the build.

There are also
`--install-dir` and `--install-prefix` arguments to provide some more
fine-grained control of the installation directories. However, given that
EdenSCM provides no compatibility guarantees between commits we generally
recommend building and installing to a temporary location, rather than
installing to the traditional system installation directories.

If you want to invoke `cmake` again to iterate on EdenFS, there is a helpful `run_cmake.py` script output in the scratch build directory.  You can find the scratch build directory from logs or with `python3 ./build/fbcode_builder/getdeps.py show-build-dir eden`

### Run tests

By default `getdeps.py` will build the tests for a manifest eden_scm. To run them:

    cd eden
    python3 ./build/fbcode_builder/getdeps.py --allow-system-packages test eden_scm

## Build with cmake directly

If you don't want to let getdeps invoke cmake for you then by default, building the tests is disabled as part of the CMake `all` target.
To build the tests, specify `-DBUILD_TESTS=ON` to CMake at configure time.

NB if you want to invoke `cmake` again to iterate on a `getdeps.py` build, there is a helpful `run_cmake.py` script output in the scratch-path build directory. You can find the scratch build directory from logs or with `python3 ./build/fbcode_builder/getdeps.py show-build-dir`

Running tests with ctests also works if you cd to the build dir, e.g. `
`(cd $(python3 ./build/fbcode_builder/getdeps.py show-build-dir) && ctest)`

## Ubuntu LTS, CentOS Stream, Fedora

Use the `getdeps.py` approach above. We test in CI on Ubuntu LTS, and occasionally on other distros.

If you find the set of system packages is not quite right for your chosen distro,  you can specify distro version specific overrides in the dependency manifests (e.g. `build/fbcode_builder/manifests/boost` ).   You could probably make it work on most recent Ubuntu/Debian or Fedora/Redhat derived distributions.

At time of writing (Feb 2022) there is a build break on GCC 11.x based systems in folly which in turn will break fbthrift and thus Eden.  Using Ubuntu 20.04 in a virtual environment is one possible workaround for this to try out the Eden tools.

## Windows

Eden CLI, EdenFS are used on Windows. `getdeps.py` would be the way to start with EdenFS having the higher chance of success, however we don't run these in github CI

Mononoke is not supported on Windows.

## macOS

`getdeps.py` builds work on macOS and are tested in CI, however if you prefer, you can try one of the macOS package managers
