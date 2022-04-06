# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_pre_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  $ blobimport repo-hg/.hg repo --derived-data-type=fsnodes

check blobstore numbers, walk will do some more steps for mappings
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ WALKABLEBLOBCOUNT=$(ls $BLOBPREFIX.* | grep -v .filenode_lookup. | wc -l)
  $ echo "$WALKABLEBLOBCOUNT"
  33
  $ find $TESTTMP/blobstore/blobs/ -type f ! -path "*.filenode_lookup.*" -exec du --bytes -c {} + | tail -1 | cut -f1
  2805

Base case, sample all in one go. Expeding WALKABLEBLOBCOUNT keys plus mappings and root.  Note that the total is 3086, but blobs are 2805. This is due to BonsaiHgMapping loading the hg changeset
  $ mononoke_new_walker -l sizing corpus -q -b master_bookmark --output-dir=full --sample-rate 1 -I deep -i default -i derived_fsnodes 2>&1 | strip_glog
  * Run */s,*/s,3086,36,0s; Type:Raw,Compressed AliasContentMapping:333,9 BonsaiHgMapping:281,3 Bookmark:0,0 Changeset:277,3 FileContent:12,3 FileContentMetadata:351,3 Fsnode:822,3 FsnodeMapping:96,3 HgBonsaiMapping:0,0 HgChangeset:281,3 HgChangesetViaBonsai:0,0 HgFileEnvelope:189,3 HgFileNode:0,0 HgManifest:444,3* (glob)

Check the corpus dumped to disk agrees with the walk stats
  $ for x in full/*; do size=$(find $x -type f -exec du --bytes -c {} + | tail -1 | cut -f1); if [[ -n "$size" ]]; then echo "$x $size"; fi; done
  full/AliasContentMapping 333
  full/BonsaiHgMapping 281
  full/Changeset 277
  full/FileContent 12
  full/FileContentMetadata 351
  full/Fsnode 822
  full/FsnodeMapping 96
  full/HgChangeset 281
  full/HgFileEnvelope 189
  full/HgManifest 444

Repeat but using the sample-offset to slice.  Offset zero will tend to be larger as root paths sample as zero. 2000+475+611=3086
  $ for i in {0..2}; do mkdir -p slice/$i; echo slice $i; mononoke_new_walker -L graph corpus -q -b master_bookmark -I deep -i default -i derived_fsnodes --output-dir=slice/$i --sample-rate=3 --sample-offset=$i 2>&1; done | strip_glog
  slice 0
  Seen,Loaded: 46,46
  * Run */s,*/s,2000,17,*s; * (glob)
  slice 1
  Seen,Loaded: 46,46
  * Run */s,*/s,475,9,*s; * (glob)
  slice 2
  Seen,Loaded: 46,46
  * Run */s,*/s,611,10,*s; * (glob)

See the breakdown
  $ for x in slice/*/*; do size=$(find $x -type f -exec du --bytes -c {} + | tail -1 | cut -f1); if [[ -n "$size" ]]; then echo "$x $size"; fi; done
  slice/0/AliasContentMapping 111
  slice/0/BonsaiHgMapping 101
  slice/0/Changeset 104
  slice/0/FileContent 4
  slice/0/FileContentMetadata 117
  slice/0/Fsnode 822
  slice/0/FsnodeMapping 32
  slice/0/HgChangeset 202
  slice/0/HgFileEnvelope 63
  slice/0/HgManifest 444
  slice/1/AliasContentMapping 111
  slice/1/BonsaiHgMapping 79
  slice/1/Changeset 69
  slice/1/FileContent 4
  slice/1/FileContentMetadata 117
  slice/1/FsnodeMapping 32
  slice/1/HgFileEnvelope 63
  slice/2/AliasContentMapping 111
  slice/2/BonsaiHgMapping 101
  slice/2/Changeset 104
  slice/2/FileContent 4
  slice/2/FileContentMetadata 117
  slice/2/FsnodeMapping 32
  slice/2/HgChangeset 79
  slice/2/HgFileEnvelope 63

Check overall total
  $ find slice -type f -exec du --bytes -c {} + | tail -1 | cut -f1
  3086

Check path regex can pick out just one path
  $ mononoke_new_walker -l sizing corpus -q -b master_bookmark --output-dir=A --sample-path-regex='^A$' --sample-rate 1 -I deep -i default -i derived_fsnodes 2>&1 | strip_glog
  * Run */s,*/s,295,6,0s; Type:Raw,Compressed AliasContentMapping:111,3 BonsaiHgMapping:0,0 Bookmark:0,0 Changeset:0,0 FileContent:4,1 FileContentMetadata:117,1 Fsnode:0,0 FsnodeMapping:0,0 HgBonsaiMapping:0,0 HgChangeset:0,0 HgChangesetViaBonsai:0,0 HgFileEnvelope:63,1 HgFileNode:0,0 HgManifest:0,0* (glob)
