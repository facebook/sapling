# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Check the count of blobstore blobs.  The walker should fetch all blobs, and duplicate
fetches should be handled by the cache.
  $ ls $TESTTMP/blobstore/blobs/blob-repo0000.* | grep -v .filenode_lookup. | wc -l
  30

Base case, sample all in one go. Expecting the same number of keys.
# TODO(mbthomas): concurrent fetches may not hit in the cache
  $ mononoke_walker -l sizing scrub -q -b master_bookmark --sample-rate 1 -I deep 2>&1 | strip_glog
  * Run */s,*/s,*,*,*s; * (glob)

Three separate cycles moving offset each time, should result in scrubing same total of bytes and keys
# TODO(mbthomas): concurrent fetches may not hit in the cache
  $ for i in {0..2}; do mononoke_walker -l sizing scrub -q -b master_bookmark -I deep --sample-rate=3 --sample-offset=$i 2>&1; done | strip_glog
  * Run */s,*/s,*,*,*s; * (glob)
  * Run */s,*/s,*,*,*s; * (glob)
  * Run */s,*/s,*,*,*s; * (glob)
