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

check blobstore numbers, walk will do some more steps for mappings
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ WALKABLEBLOBCOUNT=$(ls $BLOBPREFIX.* | grep -v .filenode_lookup. | wc -l)
  $ echo "$WALKABLEBLOBCOUNT"
  27

Full edge base case, sample all in one go.  We are excluding BonsaiHgMapping from sampling as it has no blobstore form, was being creditted with its filenode lookups
  $ mononoke_new_walker -l sizing compression-benefit -q -b master_bookmark --sample-rate 1 --exclude-sample-node-type BonsaiHgMapping -I deep 2>&1 | strip_glog
  * Run */s,*/s,1887,1858,1%,*s;* (glob)

Reduced edge base case, sample all in one go.  We are excluding several edges from the usual deep walk to force path tracking to work harder
  $ mononoke_new_walker -l sizing compression-benefit -q -b master_bookmark --sample-rate 1 --exclude-sample-node-type BonsaiHgMapping -I deep -X ChangesetToBonsaiParent -X HgFileEnvelopeToFileContent -X HgChangesetToHgParent 2>&1 | strip_glog
  * Run */s,*/s,1887,1858,1%,*s;* (glob)

Three separate cycles allowing all edges, total bytes should be the same as full edge base case
  $ for i in {0..2}; do mononoke_new_walker -l sizing compression-benefit -q -b master_bookmark -I deep --sample-rate=3 --exclude-sample-node-type BonsaiHgMapping --sample-offset=$i 2>&1; done | strip_glog
  * Run */s,*/s,1045,1016,2%,*s; * (glob)
  * Run */s,*/s,364,364,0%,*s;* (glob)
  * Run */s,*/s,478,478,0%,*s; * (glob)

Reduced edge three separate cycles moving offset each time, total in each cycle should be the same as above.
  $ for i in {0..2}; do mononoke_new_walker -l sizing compression-benefit -q -b master_bookmark -I deep -X ChangesetToBonsaiParent -X HgFileEnvelopeToFileContent -X HgChangesetToHgParent --sample-rate=3 --exclude-sample-node-type BonsaiHgMapping --sample-offset=$i 2>&1; done | strip_glog
  * Run */s,*/s,1045,1016,2%,*s; * (glob)
  * Run */s,*/s,364,364,0%,*s;* (glob)
  * Run */s,*/s,478,478,0%,*s; * (glob)
