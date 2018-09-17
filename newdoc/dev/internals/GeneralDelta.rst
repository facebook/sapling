GeneralDelta
============

Using the generaldelta compression option.

Introduction
------------

The original Mercurial compression format has a particular weakness in storing and transmitting deltas for branches that are heavily interleaved. In some instances, this can make the size of the manifest data (stored in **00manifest.d**) balloon by 10x or more. The generaldelta option is an effort to mitigate that, while still maintaining Mercurial's O(1)-bounded performance.

The generaldelta feature is available in Mercurial 1.9 and later.

Enabling generaldelta
---------------------

The generaldelta feature is enabled by default in Mercurial 3.7

For older release can be enabled for new clones with:

::

   [format]
   generaldelta = true

This will actually enable three features:

* generaldelta storage

* recomputation of delta on pull (to be stored as "optimised" general delta)

* delta reordering on pulls when this is enabled on the server side

The latter feature will let clients without generaldelta enabled experience some of the disk space and bandwidth benefits.

Converting a repo to generaldelta
---------------------------------

This is as simple as:

::

   $ hg clone -U --config format.generaldelta=1 --pull project project-generaldelta

The aforementioned reordering can also marginally improve compression for generaldelta clients, which can be tried with a second pass:

::

   $ hg clone -U --config format.generaldelta=1 --pull project-generaldelta project-generaldelta-pass2

Detailed compression statistics for the manifest can be checked with **debugrevlog**:

::

   $ hg debugrevlog -m
   format : 1
   flags  : generaldelta

   revisions     :   14932
       merges    :    1763 (11.81%)
       normal    :   13169 (88.19%)
   revisions     :   14932
       full      :      61 ( 0.41%)
       deltas    :   14871 (99.59%)
   revision size : 3197528
       full      :  744577 (23.29%)
       deltas    : 2452951 (76.71%)

   avg chain length  : 172
   compression ratio : 229

   uncompressed data size (min/max/avg) : 125 / 80917 / 49156
   full revision size (min/max/avg)     : 113 / 37284 / 12206
   delta size (min/max/avg)             : 0 / 27029 / 164

   deltas against prev  : 13770 (92.60%)
       where prev = p1  : 13707     (99.54%)
       where prev = p2  :     8     ( 0.06%)
       other            :    55     ( 0.40%)
   deltas against p1    :  1097 ( 7.38%)
   deltas against p2    :     4 ( 0.03%)
   deltas against other :     0 ( 0.00%)

Of particular interest are the number of full revisions and the average delta size.

