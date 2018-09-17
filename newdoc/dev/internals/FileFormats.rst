Mercurial File Formats
======================

This page is a starting point for understanding Mercurial file internals. Text files are marked as "local" if they are in the local charset, otherwise "utf-8" or "ascii". Files intended to be user-editable are marked "editable". All files are optional unless otherwise noted.

Bundle files
------------

Bundle files use the WireProtocol_ format, with a header indicating compression type:

* 'HG10BZ' - bzip2 compressed

* 'HG10GZ' - gzip compressed

* 'HG10UN' - uncompressed

Files in the working directory
------------------------------

.hgignore (local editable)
~~~~~~~~~~~~~~~~~~~~~~~~~~

This lists ignore patterns. May or may not be managed.

Source: mercurial/ignore.py

.hgtags (utf-8 editable)
~~~~~~~~~~~~~~~~~~~~~~~~

This is a managed file containing tags. Effective tags are determined by combining information from this file on all project heads (not from the working directory).

One line per tag in the format '<full hex changeset id> <tag>'.

Source: mercurial/tags.py

.hgsigs (ascii) [gpg]
~~~~~~~~~~~~~~~~~~~~~

This file contains changeset signatures from the gpg extension, one per line. Format:

<full hex changeset id> <signature version> <base64-encoded signature>

Source: hgext/gpg.py

.hg/
~~~~

This directory in the repository root contains all the revision history and non-versioned configuration data.

Files in the repository directory (.hg)
---------------------------------------

00changelog.i
~~~~~~~~~~~~~

In repositories with the 'store' requirement, this is a placeholder to warn older Mercurial about version mismatch.

requires (ascii)
~~~~~~~~~~~~~~~~

If this file exists, it contains one line per feature that a local Mercurial must support to read the repository.

bookmarks
~~~~~~~~~

This file stores Bookmarks_. Same format as .hgtags: one bookmark per line, in the format:

<full hex changeset id> <bookmark>

bookmarks.current
~~~~~~~~~~~~~~~~~

This file contains a single line with the active bookmark name. If there is no active bookmark, this file won't exist.

branch (utf-8)
~~~~~~~~~~~~~~

This file contains a single line with the branch name for the branch in the working directory. If it doesn't exist, the branch is \'\' (aka 'default').

Source: mercurial/dirstate.py:_branch

branch.cache (utf-8)
~~~~~~~~~~~~~~~~~~~~

First line is:

<full hex id of tip> <revision number of tip>

This line is used to determine if the cache is current.

Remaining lines are:

<full hex id of branch tip> <branch name>

dirstate
~~~~~~~~

This file contains information on the current state of the working directory in a binary format. It begins with two 20-byte hashes, for first and second parent, followed by an entry for each file. Each file entry is of the following form:

<1-byte state><4-byte mode><4-byte size><4-byte mtime><4-byte name length><n-byte name>

If the name contains a null character, it is split into two strings, with the second being the copy source for move and copy operations.

If the dirstate file is not present, parents are assumed to be (null, null) with no files tracked.

Source: mercurial/parsers.py:parse_dirstate()

hgrc (local editable)
~~~~~~~~~~~~~~~~~~~~~

This is the repository-local configuration file.

inotify.sock [inotify]
~~~~~~~~~~~~~~~~~~~~~~

This is a socket created by the inotify extension to communicate with its daemon.

localtags (local editable)
~~~~~~~~~~~~~~~~~~~~~~~~~~

File containing local tags. Same format as .hgtags.

patches/ [mq]
~~~~~~~~~~~~~

This directory contains mq's patch management data

wlock
~~~~~

The lock file for the working directory state

Source: mercurial/lock.py, mercurial/localrepo.py:wlock()

journal.*
~~~~~~~~~

These files are backups of files before the beginning of a transaction used to restore earlier state on failure:

* journal.dirstate - copy of dirstate

* journal.branch - copy of branch

undo.*
~~~~~~

Files from last transaction to allow rollback

* undo.dirstate - copy of dirstate

* undo.branch - copy of branch

Files in the repository store (.hg or .hg/store)
------------------------------------------------

The following files are stored under .hg/store in repos with the store requirement, otherwise in .hg

lock
~~~~

The lock file for the repository store

Source: mercurial/lock.py, mercurial/localrepo.py:lock()

journal
~~~~~~~

The journal file is a text file containing one entry per line of the form:

<filename> <pre-modified length>

This file allows mercurial to undo changes to revlogs. If this file exists, a transaction is in progress or has been interrupted.

Source: mercurial/transaction.py

undo
~~~~

Renamed journal to allow rollback after transaction is complete.

Source: mercurial/localrepo.py:rollback()

00changelog.[id]
~~~~~~~~~~~~~~~~

The project changelog, stored in revlog format.

Source: mercurial/changelog.py

00manifest.[id]
~~~~~~~~~~~~~~~

The project manifest, stored in revlog format. Each manifest revision contains a list of the file revisions in each changeset, in the form:

<filename>\0<hex file revision id>[<flags>]\n

Source: mercurial/parsers.c:parse_manifest()

fncache
~~~~~~~

For the fncache repository format Mercurial maintains a new file 'fncache' (thus the name of the format) inside '.hg/store'. The fncache file contains the paths of all filelog files in the store as encoded by mercurial.filelog.encodedir. The paths are separated by '\n' (LF).

data/
~~~~~

Revlogs for each file in the project history. Names are escaped in various increasingly-complex ways:

* old (see mercurial/filelog.py:encodedir()):

  * directory names ending in .i or .d have .hg appended

* store (see mercurial/store.py:encodedstore):

  * uppercase is escaped: 'FOO' -> '_f_o_o'

  * character codes outside of 32-126 are converted to '~XX' hex format

* fncache (see mercurial/store.py:hybridencode):

  * windows reserved filename prefixes are ~XX-encoded

  * very long filenames and stored by hash

Metadata may be stored at the start of each revision in a file revlog. If any metadata is there, the file contents will start with '\1\n', after which an arbitrary list of metadata pairs will follow, in '%k: %v\n' format. After that, another '\1\n' sequence follows to denote the start of the content.

-------------------------

 CategoryInternals_

.. ############################################################################

.. _WireProtocol: WireProtocol

.. _Bookmarks: Bookmarks

