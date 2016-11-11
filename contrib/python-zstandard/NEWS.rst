Version History
===============

0.5.0 (released 2016-11-10)
---------------------------

* Vendored version of zstd updated to 1.1.1.
* Continuous integration for Python 3.6 and 3.7
* Continuous integration for Conda
* Added compression and decompression APIs providing similar interfaces
  to the standard library ``zlib`` and ``bz2`` modules. This allows
  coding to a common interface.
* ``zstd.__version__` is now defined.
* ``read_from()`` on various APIs now accepts objects implementing the buffer
  protocol.
* ``read_from()`` has gained a ``skip_bytes`` argument. This allows callers
  to pass in an existing buffer with a header without having to create a
  slice or a new object.
* Implemented ``ZstdCompressionDict.as_bytes()``.
* Python's memory allocator is now used instead of ``malloc()``.
* Low-level zstd data structures are reused in more instances, cutting down
  on overhead for certain operations.
* ``distutils`` boilerplate for obtaining an ``Extension`` instance
  has now been refactored into a standalone ``setup_zstd.py`` file. This
  allows other projects with ``setup.py`` files to reuse the
  ``distutils`` code for this project without copying code.
* The monolithic ``zstd.c`` file has been split into a header file defining
  types and separate ``.c`` source files for the implementation.

History of the Project
======================

2016-08-31 - Zstandard 1.0.0 is released and Gregory starts hacking on a
Python extension for use by the Mercurial project. A very hacky prototype
is sent to the mercurial-devel list for RFC.

2016-09-03 - Most functionality from Zstandard C API implemented. Source
code published on https://github.com/indygreg/python-zstandard. Travis-CI
automation configured. 0.0.1 release on PyPI.

2016-09-05 - After the API was rounded out a bit and support for Python
2.6 and 2.7 was added, version 0.1 was released to PyPI.

2016-09-05 - After the compressor and decompressor APIs were changed, 0.2
was released to PyPI.

2016-09-10 - 0.3 is released with a bunch of new features. ZstdCompressor
now accepts arguments controlling frame parameters. The source size can now
be declared when performing streaming compression. ZstdDecompressor.decompress()
is implemented. Compression dictionaries are now cached when using the simple
compression and decompression APIs. Memory size APIs added.
ZstdCompressor.read_from() and ZstdDecompressor.read_from() have been
implemented. This rounds out the major compression/decompression APIs planned
by the author.

2016-10-02 - 0.3.3 is released with a bug fix for read_from not fully
decoding a zstd frame (issue #2).

2016-10-02 - 0.4.0 is released with zstd 1.1.0, support for custom read and
write buffer sizes, and a few bug fixes involving failure to read/write
all data when buffer sizes were too small to hold remaining data.

2016-11-10 - 0.5.0 is released with zstd 1.1.1 and other enhancements.
