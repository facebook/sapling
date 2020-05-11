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

Base case, sample all in one go. Expeding WALKABLEBLOBCOUNT keys plus mappings and root.
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark --sample-rate 1 -I deep -x BonsaiFsnodeMapping -x Fsnode 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,2168,30,*s; Type:Raw,Compressed AliasContentMapping:333,9 BonsaiChangeset:277,3 BonsaiHgMapping:281,3 Bookmark:0,0 FileContent:12,3 FileContentMetadata:351,3 HgBonsaiMapping:0,0 HgChangeset:281,3 HgFileEnvelope:189,3 HgFileNode:0,0 HgManifest:444,3* (glob)
  Walked/s,* (glob)

Three separate cycles moving offset each time, should result in scrubing same total of bytes (728+857+583=2168) and keys (10+14+6=30)
  $ for i in {0..2}; do mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I deep -x BonsaiFsnodeMapping -x Fsnode --sample-rate=3 --sample-offset=$i 2>&1; done | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,728,10,*s; Type:Raw,Compressed AliasContentMapping:74,2 BonsaiChangeset:104,1 BonsaiHgMapping:101,1 Bookmark:0,0 FileContent:4,1 FileContentMetadata:117,1 HgBonsaiMapping:0,0 HgChangeset:202,2 HgFileEnvelope:126,2 HgFileNode:0,0 HgManifest:0,0* (glob)
  Walked/s,* (glob)
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,857,14,*s; Type:Raw,Compressed AliasContentMapping:222,6 BonsaiChangeset:69,1 BonsaiHgMapping:79,1 Bookmark:0,0 FileContent:8,2 FileContentMetadata:234,2 HgBonsaiMapping:0,0 HgChangeset:0,0 HgFileEnvelope:0,0 HgFileNode:0,0 HgManifest:245,2* (glob)
  Walked/s,* (glob)
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Run */s,*/s,583,6,*s; Type:Raw,Compressed AliasContentMapping:37,1 BonsaiChangeset:104,1 BonsaiHgMapping:101,1 Bookmark:0,0 FileContent:0,0 FileContentMetadata:0,0 HgBonsaiMapping:0,0 HgChangeset:79,1 HgFileEnvelope:63,1 HgFileNode:0,0 HgManifest:199,1* (glob)
  Walked/s,* (glob)
