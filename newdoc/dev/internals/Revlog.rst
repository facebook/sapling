Revlog
======

.. note::

   A new revlog format was introduced for Mercurial 0.9: see* RevlogNG_ 

A **revlog**, for example ``.hg/data/somefile.d``, is the most important data structure and represents all versions of a file in a repository.  Each version is stored compressed in its entirety or stored as a compressed binary delta (difference) relative to the preceeding version in the revlog. Whether to store a full version is decided by how much data would be needed to reconstruct the file.  This system ensures that Mercurial does not need huge amounts of data to reconstruct any version of a file, no matter how many versions are stored.

The reconstruction requires a single read, if Mercurial knows when and where to read.  Each revlog therefore has an **index**, for example ``.hg/store/data/somefile.i``, containing one fixed-size record for each version.  The record contains:

* the nodeid of the file version

* the nodeids of its parents

* the length of the revision data

* the offset in the revlog saying where to begin reading

* the base of the delta chain

* the linkrev pointing to the corresponding changeset

Here's an example:

::

   $ hg debugindex .hg/store/data/README.i
      rev    offset  length   base linkrev nodeid       p1           p2
        0         0    1125      0       0 80b6e76643dc 000000000000 000000000000
        1      1125     268      0       1 d6f755337615 80b6e76643dc 000000000000
        2      1393      49      0      27 96d3ee574f69 d6f755337615 000000000000
        3      1442     349      0      63 8e5de3bb5d58 96d3ee574f69 000000000000
        4      1791      55      0      67 ed9a629889be 8e5de3bb5d58 000000000000
        5      1846     100      0      81 b7ac2f914f9b ed9a629889be 000000000000
        6      1946     405      0     160 1d528b9318aa b7ac2f914f9b 000000000000
        7      2351      39      0     176 2a612f851a95 1d528b9318aa 000000000000
        8      2390       0      0     178 95fdb2f5e08c 2a612f851a95 2a612f851a95
        9      2390     127      0     179 fc5dc12f851b 95fdb2f5e08c 000000000000
       10      2517       0      0     182 24104c3ccac4 fc5dc12f851b fc5dc12f851b
       11      2517     470      0     204 cc286a25cf37 24104c3ccac4 000000000000
       12      2987     346      0     205 ffe871632da6 cc286a25cf37 000000000000
   ...

With one read of the index to fetch the record and then one read of the revlog, Mercurial can reconstruct any version of a file in time proportional to the file size.

So that adding a new version requires only O(1) seeks, the revlogs and their indices are append-only.

Revlogs are also used for manifests and changesets.

.. _RevlogNG: RevlogNG

