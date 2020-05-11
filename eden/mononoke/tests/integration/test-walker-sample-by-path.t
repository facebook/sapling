# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

check blobstore numbers, walk will do some more steps for mappings
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ WALKABLEBLOBCOUNT=$(ls $BLOBPREFIX.* | grep -v .filenode_lookup. | wc -l)
  $ echo "$WALKABLEBLOBCOUNT"
  27

Full edge base case, sample all in one go.  We are excluding several edges
  $ mononoke_walker --storage-id=blobstore --readonly-storage compression-benefit -q --bookmark master_bookmark --sample-rate 1 -I deep -x BonsaiFsnodeMapping -x Fsnode 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,2168,2139,1%,*s;* (glob)
  Walked/s,* (glob)

Reduced edge base case, sample all in one go.  We are excluding several edges from the usual deep walk to force path tracking to work harder
  $ mononoke_walker --storage-id=blobstore --readonly-storage compression-benefit -q --bookmark master_bookmark --sample-rate 1 -I deep -x BonsaiFsnodeMapping -x Fsnode -X BonsaiChangesetToBonsaiParent -X HgFileEnvelopeToFileContent -X HgChangesetToHgParent 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,2168,2139,1%,*s;* (glob)
  Walked/s,* (glob)

Three separate cycles allowing all edges, total bytes should be the same as full edge base case
  $ for i in {0..2}; do mononoke_walker --storage-id=blobstore --readonly-storage compression-benefit -q --bookmark master_bookmark -I deep -x BonsaiFsnodeMapping -x Fsnode --sample-rate=3 --sample-offset=$i 2>&1; done | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,1146,1117,2%,*s; * (glob)
  Walked/s,* (glob)
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,443,443,0%,*s;* (glob)
  Walked/s,* (glob)
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,579,579,0%,*s; * (glob)
  Walked/s,* (glob)

Reduced edge three separate cycles moving offset each time, total in each cycle should be the same as above.
  $ for i in {0..2}; do mononoke_walker --storage-id=blobstore --readonly-storage compression-benefit -q --bookmark master_bookmark -I deep -x BonsaiFsnodeMapping -x Fsnode -X BonsaiChangesetToBonsaiParent -X HgFileEnvelopeToFileContent -X HgChangesetToHgParent --sample-rate=3 --sample-offset=$i 2>&1; done | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,1146,1117,2%,*s; * (glob)
  Walked/s,* (glob)
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,443,443,0%,*s;* (glob)
  Walked/s,* (glob)
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,579,579,0%,*s; * (glob)
  Walked/s,* (glob)
