# Eden

Eden is a project with several components, the most prominent of which is a virtual filesystem
built using FUSE.

## Caveat Emptor

Eden is still in early stages of development. We are making it available now because we plan to
start making references to it from our other open source projects, such as
[Buck](https://github.com/facebook/buck), [Watchman](https://github.com/facebook/watchman), and
[Nuclide](https://github.com/facebook/nuclide).

**The version that we provide on GitHub does not build yet**.

This is because the code is exported verbatim from an internal repository at Facebook, and
not all of the scaffolding from our internal repository can be easily extracted. The key areas
where we need to shore things up are:

* The reinterpretations of build macros in `DEFS`.
* A process for including third-party dependencies (presumably via Git submodules) and wiring up the
`external_deps` argument in the build macros to point to them.
* Providing to toolchain needed to power the [undocumented] `thrift_library()` rule in Buck.

The goal is to get Eden building on both Linux and OS X, though Linux support is expected to come
first.
