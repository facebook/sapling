Handling Large Files
====================

Mercurial's internal file-handling limitations.

Mercurial handles files in memory
---------------------------------

The primary memory constraint in Mercurial is the **size of the largest single file being managed**. This is because Mercurial is designed for  source code, not bulk data, and it's an order of magnitude faster and easier to handle entire files in memory than to manage them piecewise.

Current internal limits on Mercurial file size
----------------------------------------------

* all files are currently handled as single Python strings **in memory** 

* the diff/delta algorithm currently assumes two revisions + line indexes + resulting diff can fit in memory (so **3-5x** overhead)

* compression and decompression may use 2x memory

* the patch algorithm assumes original + all patches + output can fit in memory

* individual deltas are limited to 2G in the revlog index

* uncompressed revision length is also limited to 2G in the index

* revlogs have a maximum length of 48 bits (256 terabytes)

* dirstate only tracks file sizes up to 2G

* the wire protocol and bundle format currently have 2G limits on individual deltas

Platform limits
---------------

* 32-bit applications have a limited amount of **address space** regardless of available system memory

  * 2G on Windows

  * 3G on Linux

* The large allocations discussed above will cause address space fragmentation, which means much less than 2G will be usable

Thus, 32-bit versions of Mercurial on Windows may run into trouble with single files in the neighborhood of **400MB**. 32-bit Linux executables can typically handle files up to around **1GB** with sufficient RAM and swap.

64-bit Mercurial will instead hit the internal **2GB** barriers.

Future Directions
-----------------

With some small changes, the 2GB barriers can probably be pushed back to 4GB. By changing some index and protocol structures, we can push this back to terabytes, but you'll need a corresponding amount of RAM+swap to handle those large files.

To go beyond this limit and handle files much larger than available memory, we would need to do some fairly substantial replumbing of Mercurial's internals. This is desirable for handling extremely large files (video, astronomical data, ASIC design) and reducing requirements for web servers. Possible approaches to handling larger files:

* use mmap to back virtual memory with disk

* use a 'magic string' class to transparently bring portions of a file

    into memory on demand

* use iterable string vectors for all file contents

The mmap approach doesn't really help as we quickly run into a 3GB barrier on 32-bit machines.

The magic string technique would require auditing every single use of the string to avoid things like write() that would instantiate the whole string in memory.

If we instead declare that we pass all file contents around as an iterable (list, tuple, or iterator) of large multi-megabyte string fragments, every user will break loudly and need replacing with an appropriate loop, thus simplifying the audit process. This concept can be wrapped in a simple class, but it can't have any automatic conversion to 'str' type. As a first pass, making everything work with one-element lists should be easy.

Fixing up the code:

The mpatch code can be made to work on a window without too much effort, but it may be hard to avoid degrading to O(n²) performance overall as we iterate through the window.

The core delta algorithm could similarly be made to delta corresponding chunks of revisions, or could be extended to support a streaming binary diff.

Changing compression and decompression to work on iterables is trivial. Adjusting most I/O is also trivial. Various operations like annotate will be harder.

Extending dirstate and revlog chunks to 4G means going to unsigned pack/unpack specifiers, which is easy enough. Beyond that, more invasive format changes will be needed.

If revlog is changed to store the end offset of each hunk, the compressed hunk length needn't be stored. This will let us go to 48-bit uncompressed lengths and 64-bit total revlogs without enlarging the index.

