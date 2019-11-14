Encoding Strategy
=================

How encoding works in the Mercurial codebase.

Overview
--------

There are three types of string used in Mercurial:

* byte string in unknown encoding (tracked data)

* byte string in local encoding (messages, user input)

* byte string in UTF-8 encoding (repository metadata)

This page sorts out which type of string can be expected where on disk and in the code and what functions manipulate them.

Platform issues
---------------

Linux and Unix
~~~~~~~~~~~~~~

* kernel and native filesystems are encoding-transparent

* filesystem APIs are UTF-8-compatible, but accept arbitrary encodings

* multiple encodings can exist for file names on the same system

* UTF-8 is the defacto standard console and text file on modern systems, though other encodings are still common

* UTF-8 filenames are generally in NFC, but this is not enforced and NFD-normalized names are treated as different files

* common tools like '``make(1)``' are designed to assume file encoding and contents match, so UTF-8 filenames in files will be used to find UTF-8 filenames on disk

Mac OS X
~~~~~~~~

* Kernel is encoding-transparent

* Native HFS+ filesystem encodes filenames in UTF-16, kernel translates to UTF-8 for ANSI C APIs

* Filenames are normalized to (approximately) NFD, opening by an NFC name works

* Filesystem uses a Unicode-aware case-folding algorithm by default

* Kernel I/O interfaces accept UTF-8 filenames

* Non-UTF-8 filenames are %-escaped on open(), but not on listdir

* Most tools assume file encoding and contents match

* UTF-8 is the defacto standard console and text file encoding

* Legacy tools may use MacRoman

Windows
~~~~~~~

* Kernel has several different incompatible 8-bit encoding regimes:

  * default encoding used in the GUI

  * default encoding used in the filesystem

  * default (legacy) encoding used in the console (cmd.exe)

* Kernel has a mix of byte-width and wide character APIs

* Kernel and console environment have basically no support for UTF-8 filename I/O or character display

* Kernel may fold non-ASCII filenames to fit in the current codepage with a *one-way* best-fit algorithm (ie reported files can't actually be opened!)

* Filesystem uses a highly-obscure Unicode-aware case-folding algorithm by default

* Filenames are generally in Unicode NFC, but this is not enforced and NFD-normalized names are treated as different files

* Many tools attempt to do transcoding of file contents from the local encoding to UTF-16 before passing it off to the filesystem

* UTF-16 text files are occasionally found

* A couple multibyte character encodings like Shift-JIS cause trouble here because they make the "\" byte ambiguous

* Console has limited support for UTF-8 in codepage 65001, but is generally buggy

Web and XML
~~~~~~~~~~~

* URLs are encoded in ASCII with %-escaping to ISO-Latin, according to RFC1738

* translation of URLs to filesystem paths is webserver-dependent

* HTML defaults to ISO-Latin, but may contain encoding specifiers or Unicode entities

* XML assumes a subset of UTF-8

Mercurial assumptions
~~~~~~~~~~~~~~~~~~~~~

* non-ASCII filenames are not reliably portable between systems in general

* the "makefile issue" (whereby Unix handles filenames as bytes, rather than text) means that in general, we must attempt to preserve filename encoding

* On Windows, we prefer the 8-bit encoding of the GUI environment to that of the console to be compatible with typical editors

Problems and constraints
------------------------

The encoding tracking problem
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

There are a number of problems with detecting and tracking encoding of files:

* With the exception of UTF-8 and ASCII, encodings cannot be reliably detected

* Files may not be in a known or valid encoding, or may be using multiple encodings

* Experience has shown that users will not correctly configure encodings until after a problem is created in permanent history

* Conversion to and from Unicode may not perfectly preserve file contents

* Many projects may intentionally contain files in different encodings, so localizing encoding will be unhelpful

Therefore, we do not attempt to convert file contents to match user locales and instead preserve files intact.

The "makefile problem"
~~~~~~~~~~~~~~~~~~~~~~

The "makefile problem" is a generic name for the general issue that:

* most software project involve multiple files

* those files contain **text**, but refer to each other by name (for instance, via a makefile, include directives, URLs, manifests, IDE settings file, etc.)

* some operating systems treat filenames as consisting of **bytes**, rather than text.

For inter-file references to work, the filesystem, file contents, and tool must all agree on how to handle filenames. This is, unfortunately, completely precluded by the fact that Unix and Windows fundamentally disagree on filename handling. Unix treats filenames as bytes, while Windows treats them as character strings. **This is, fundamentally, why there exists no general, portable solution to the makefile problem.**

Consider the following case:

* makefile mentions file 'á' in UTF-8 (bytes C3 A1)

* almost all tools with a Unix background will looks up file 'á' on the file system with the exact bytes C3 A1

* some (but not all) Windows tools will look up the file with the bytes 1e 00

There are two incompatible strategies to approach this problem and no middle ground. Therefore, Mercurial has intentionally chosen a strategy that works well for some tools (mostly Unix ones) and works less well for others (mostly Windows ones).

On Unix, if a file refers to another file by name, it must do so using an encoding that matches the filesystem's encoding. For instance, if a filename is encoded in Latin1, a makefile must also encode that filename in Latin1. Otherwise, a compiler will fail to find the referenced file.

Therefore, Mercurial does not change filename encoding to match the locale of different users, as Unix-style tools will fail.

Unknown byte strings
--------------------

The following are explicitly treated as binary data in an unknown encoding:

* file contents

* file names

These items should be treated as binary data and preserved losslessly wherever possible. Generally speaking, it is impossible to reliably and uniquely identify file type and encoding, thus Mercurial does not attempt to distinguish 'binary' files from 'text' files when storing them and instead aims to always preserve them exactly.

Similarly, for historical reasons, non-ASCII filenames are not necessarily portable from Unix to Windows, and Mercurial does not attempt to 'solve' this problem with transcoding either.

In general, do not attempt to transcode such data to Unicode and back in Mercurial, it *will* result in data loss.

UTF-8 strings
-------------

UTF-8 strings are used to store most repository metadata. Unlike repository contents, repository metadata is 'owned and managed' by Mercurial and can be made to conform to its rules. In particular, this includes:

* commit messages stored in the changelog

* user names

* tags

* branches

The following files are stored in UTF-8:

* .hgtags

* .hg/branch

* .hg/branchheads.cache

* .hg/tags.cache

* .hg/bookmarks

These are converted to and from local strings in the relevant I/O functions, so that internally the above items are always represented in the local encoding. This restricts UTF-8-aware code to the smallest footprint possible so that the bulk of the code does not need to keep track of what encoding a string is in.

Local strings
-------------

Strings not mentioned above are generally assumed to be in the local charset encoding. This includes:

* command line arguments

* configuration files like ``.hgrc``

* prompt input

* commit message

* .hg/localtags

All user input in the form of command line arguments, configuration files, etc. are assumed to be in the local encoding.

Internal messages
~~~~~~~~~~~~~~~~~

All internal messages are written in ASCII, which is assumed to be a subset of the local encoding. Where localized string data is available, these strings are translated to the local encoding via gettext.

Mixing output
-------------

Mercurial frequently mixes output of all three varieties. For instance, the output of '``hg log -p``' will contain internal strings in local encoding to mark fields, UTF-8 metadata, and file contents in an unknown encoding. These are managed as follows:

* UTF-8 data is converted to local encoding at the earliest opportunity, generally at read time

* internal ASCII strings are translated to local encoding via gettext() or passed unmodified

* data in unknown encoding (file contents and filenames) are treated as already being in the local encoding for I/O purposes

* resulting strings are combined with typical string formatting and I/O operations

* raw binary output is used with no additional transcoding

Thus, the vast bulk of string operations in Mercurial are done *as if* they were operating on local strings.

As an example, attempts to view a patch containing UTF-8 characters on a non-UTF-8 terminal may not be entirely human-readable, but the generated patch will be *correct* in the sense that a standard patch tool will be able to apply it and get the right UTF-8 characters in the result. Similarly, '``hg cat``' of a binary file will output an exact copy of the binary file, regardless of the current encoding.

Functions
---------

The ``encoding`` module defines the following functions:

* ``fromlocal()``: converts a string from the local encoding to UTF-8 for storage **with validation**

* ``tolocal()``: converts string stored as UTF-8 to the local encoding **replacing unknown glyphs**

* ``colwidth()``: calculate the width of a local string in terminal columns

Also, ``encoding.encoding`` specifies Mercurial's idea of what the current encoding is.

Round-trip conversion
---------------------

Some data, such as branch names, are stored locally as UTF-8, read in for processing, then stored in the repository history as UTF-8 again.

This presents difficulties, as we either need to make sure the dozens of places that handle branch names do so in UTF-8 or we need to avoid conversion loss when converting from the local encoding back to UTF-8. In Mercurial post-1.7, this is facilitated by the ``encoding.localstr`` class returned by ``tolocal`` which caches the original UTF-8 version of a string alongside its local encoding. The ``fromlocal`` function can retrieve this string if it's available, which allows lossless round-trip conversion.

String operations (eg strip()) on localstr objects will lose the cached UTF-8 data.

Unicode strings
---------------

Python Unicode objects are only used in the implementation of the above functions and are carefully avoided elsewhere. Do not pass Unicode objects to any Mercurial APIs. Due to Python's misguided automatic Unicode to byte string conversion, Unicode objects are likely to work in testing, but break as soon as they encounter a non-ASCII character.

Filename strategy compatibility matrices
----------------------------------------

This section discusses different strategies of filename storage and their failure modes. The rows indicate filename and contents stored in a repo (Latin1  means "some filenames with Latin1 characters, with file contents also encoded in Latin1) while the columns indicate client operating system and configuration (read Windows Latin1 as codepage 1252, we ignore the differences here for simplicity).

Key
~~~

* Unix = Linux and other traditional Unixlike systems

* UTF-8/16 = UTF-8 file names with UTF-16 contents

* Various = multiple, unknown, or meaningless encodings

* OK = fully interoperable

* FF = fails for some filenames

* FC = fails checkout

* XX = can't even check-in

* R = human readability issues (aka mojibake)

* B? = "make problem" with native byte-oriented tools

* C? = "make problem" with native character-oriented tools

* * = Windows has limited support for UTF-8 (CP65001)

Mercurial strategy:
~~~~~~~~~~~~~~~~~~~~~~~~~~

Current versions of Mercurial read and write filenames "as-is" with no attempt to adapt to local encoding or use wide character interfaces.

==========  =============  ==============  =============  ============  =================  ===================  ==================
Encoding    Unix ASCII     Unix Latin1     Unix UTF-8     Mac UTF-8     Windows Latin1     Windows ShiftJIS     Windows UTF-8*     
ASCII       OK             OK              OK             OK            OK                 OK                   OK     
Latin1      R              OK              R              R             OK                 FF RC?               RC?     
ShiftJIS    R              R               R              R             RC?                OK                   RC?     
UTF-8       R              R               OK             OK            RC?                FF RC?               OK     
UTF-8/16    RB?            RB?             RB?            RB?           RC?                RC?                  OK     
Various     R              R               R              R             R                  R                    R     
==========  =============  ==============  =============  ============  =================  ===================  ==================

"Transcode everything to/from Unicode and use Windows Unicode API" strategy:
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Some other SCMs (SVN, Bazaar) attempt to trancode all filenames to/from Unicode internally. As file *contents* are not transcoded, files committed in with Latin1 contents are checked out in Latin1 contents.

==========  =============  ==============  =============  ============  =================  ===================  ==================
Encoding    Unix ASCII     Unix Latin1     Unix UTF-8     Mac UTF-8     Windows Latin1     Windows ShiftJIS     Windows UTF-8*     
ASCII       OK             OK              OK             OK            OK                 OK                   OK       
Latin1      FC             OK              B?             B?            OK                 C?                   C?     
ShiftJIS    FC             FC              B?             B?            C?                 OK                   C?     
UTF-8       FC             FF B?           OK             OK            OK                 OK                   OK       
UTF-8/16    FC             FF B?           B?             B?            OK                 OK                   OK       
Various     XX             XX              XX             XX            XX                 XX                   XX      
==========  =============  ==============  =============  ============  =================  ===================  ==================

Future hybrid strategy:
~~~~~~~~~~~~~~~~~~~~~~~

A proposed future version of Mercurial would use Windows Unicode APIs whenever UTF-8 filenames were stored in a repo:

==========  =============  ==============  =============  ============  =================  ===================  ==================
Encoding    Unix ASCII     Unix Latin1     Unix UTF-8     Mac UTF-8     Windows Latin1     Windows ShiftJIS     Windows UTF-8*     
ASCII       OK             OK              OK             OK            OK                 OK                   OK       
Latin1      R              OK              R              R             OK                 RC?                  RC?     
ShiftJIS    R              R               R              R             RC?                OK                   RC?     
UTF-8       R              R               OK             OK            OK                 OK                   OK       
UTF-8/16    RB?            RB?             RB?            RB?           OK                 OK                   OK       
Various     R              R               R              R             R                  R                    R     
==========  =============  ==============  =============  ============  =================  ===================  ==================

Observations
~~~~~~~~~~~~

* ASCII is the only perfectly cross-platform strategy

* Only using Windows, only using Unix, or configure all clients with the same character set also works

* Mercurial strategy almost always results in a successful checkout

* Mercurial strategy avoids the makefile problem well on Unix-like systems

* Mercurial strategy and UTF-8 encoding already works well with modern UTF-8 systems

* "Transcode" strategy trades a few successes on Windows for lots of failed checkouts elsewhere

* "Transcode" strategy can't handle "various" at all

* "Transcode" strategy sometimes trades readability problems (easy to ignore) for "makefile problems" (break the build)

* "Transcode" strategy trades some "makefile problems" for others

* Overall, "trancode" strategy is less robust and Unix-hostile

* Hybrid strategy combines upside of "transcode strategy" without introducing new failure modes.

* Hybrid with UTF-8 is nearly completely cross-platform

* UTF-16 file contents is a bad match for non-Windows systems

Historical note
---------------

Early versions of Mercurial made no effort to transcode metadata, so the ``tolocal()`` function has some fallbacks to allow guessing the encoding of strings that don't appear to be Unicode.


