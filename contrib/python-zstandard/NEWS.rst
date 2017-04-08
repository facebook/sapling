Version History
===============

0.8.1 (released 2017-04-08)
---------------------------

* Add #includes so compilation on OS X and BSDs works (#20).

0.8.0 (released 2017-03-08)
---------------------------

* CompressionParameters now has a estimated_compression_context_size() method.
  zstd.estimate_compression_context_size() is now deprecated and slated for
  removal.
* Implemented a lot of fuzzing tests.
* CompressionParameters instances now perform extra validation by calling
  ZSTD_checkCParams() at construction time.
* multi_compress_to_buffer() API for compressing multiple inputs as a
  single operation, as efficiently as possible.
* ZSTD_CStream instances are now used across multiple operations on
  ZstdCompressor instances, resulting in much better performance for
  APIs that do streaming.
* ZSTD_DStream instances are now used across multiple operations on
  ZstdDecompressor instances, resulting in much better performance for
  APIs that do streaming.
* train_dictionary() now releases the GIL.
* Support for training dictionaries using the COVER algorithm.
* multi_decompress_to_buffer() API for decompressing multiple frames as a
  single operation, as efficiently as possible.
* Support for multi-threaded compression.
* Disable deprecation warnings when compiling CFFI module.
* Fixed memory leak in train_dictionary().
* Removed DictParameters type.
* train_dictionary() now accepts keyword arguments instead of a
  DictParameters instance to control dictionary generation.

0.7.0 (released 2017-02-07)
---------------------------

* Added zstd.get_frame_parameters() to obtain info about a zstd frame.
* Added ZstdDecompressor.decompress_content_dict_chain() for efficient
  decompression of *content-only dictionary chains*.
* CFFI module fully implemented; all tests run against both C extension and
  CFFI implementation.
* Vendored version of zstd updated to 1.1.3.
* Use ZstdDecompressor.decompress() now uses ZSTD_createDDict_byReference()
  to avoid extra memory allocation of dict data.
* Add function names to error messages (by using ":name" in PyArg_Parse*
  functions).
* Reuse decompression context across operations. Previously, we created a
  new ZSTD_DCtx for each decompress(). This was measured to slow down
  decompression by 40-200MB/s. The API guarantees say ZstdDecompressor
  is not thread safe. So we reuse the ZSTD_DCtx across operations and make
  things faster in the process.
* ZstdCompressor.write_to()'s compress() and flush() methods now return number
  of bytes written.
* ZstdDecompressor.write_to()'s write() method now returns the number of bytes
  written to the underlying output object.
* CompressionParameters instances now expose their values as attributes.
* CompressionParameters instances no longer are subscriptable nor behave
  as tuples (backwards incompatible). Use attributes to obtain values.
* DictParameters instances now expose their values as attributes.

0.6.0 (released 2017-01-14)
---------------------------

* Support for legacy zstd protocols (build time opt in feature).
* Automation improvements to test against Python 3.6, latest versions
  of Tox, more deterministic AppVeyor behavior.
* CFFI "parser" improved to use a compiler preprocessor instead of rewriting
  source code manually.
* Vendored version of zstd updated to 1.1.2.
* Documentation improvements.
* Introduce a bench.py script for performing (crude) benchmarks.
* ZSTD_CCtx instances are now reused across multiple compress() operations.
* ZstdCompressor.write_to() now has a flush() method.
* ZstdCompressor.compressobj()'s flush() method now accepts an argument to
  flush a block (as opposed to ending the stream).
* Disallow compress(b'') when writing content sizes by default (issue #11).

0.5.2 (released 2016-11-12)
---------------------------

* more packaging fixes for source distribution

0.5.1 (released 2016-11-12)
---------------------------

* setup_zstd.py is included in the source distribution

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
