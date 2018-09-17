RevlogNG
========

RevlogNG was introduced with Mercurial 0.9 (see also Revlog_).

Deficiencies in original revlog format
--------------------------------------

* no uncompressed revision size stored

* SHA1 hash is potentially too weak

* compression context for deltas is often too small

* offset range is limited to 4GB

* some metadata is indicated by escaping in the data

The original index format was:

* 4 bytes: offset

* 4 bytes: compressed length

* 4 bytes: base revision

* 4 bytes: link revision

* 20 bytes: parent 1 nodeid

* 20 bytes: parent 2 nodeid

* 20 bytes: nodeid

* **76 bytes total**

RevlogNG format
---------------

* 6 bytes: offset -- This is how far into the data file we need to go to find the appropriate delta

* 2 bytes: flags

* 4 bytes: compressed length -- Once we are offset into the data file, this is how much we read to get the delta

* 4 bytes: uncompressed length -- This is just an optimization. It's the size of the file at this revision

* 4 bytes: base revision -- The last revision where the entire file is stored.

* 4 bytes: link revision -- Another optimization. Which revision is this? Which commit is this?

* 4 bytes: parent 1 revision -- Revision of parent 1 (e.g., 12, 122)

* 4 bytes: parent 2 revision -- Revision of parent 2

* 32 bytes: nodeid -- A unique identifier, also used in verification (hash of content + parent IDs)

* **64 bytes total**

RevlogNG header
~~~~~~~~~~~~~~~

As the offset of the first data chunk is always zero, the first 4 bytes (part of the offset) are used to indicate revlog version number and flags. all values are in big endian format.

RevlogNG also supports interleaving of index and data. This can greatly reduce storage overhead for smaller revlogs. In this format, the data chunk immediately follows its index entry. The position of the next index entry is calculated by adding the compressed length to the offset.

For how renames are stored see `Problems extracting renames`_ from the Mercurial mailing list.


.. ############################################################################

.. _Revlog: Revlog

.. _Problems extracting renames: http://selenic.com/pipermail/mercurial/2008-February/017139.html

