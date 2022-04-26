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

compression-benefit, file content only, not expecting any compression from the tiny test files
  $ mononoke_walker -l sizing compression-benefit -q --bookmark master_bookmark --sample-rate 1 --include-sample-node-type FileContent 2>&1 | strip_glog
  Raw/s,Compressed/s,Raw,Compressed,%Saving; Delta */s,*/s,12,12,0%* (glob)

compression-benefit, all compressible types
  $ mononoke_walker -l sizing compression-benefit -q --bookmark master_bookmark --sample-rate 1 2>&1 | strip_glog
  * Run */s,*/s,2168,2139,1%,*s; Type:Raw,Compressed,%Saving AliasContentMapping:333,333,0% BonsaiHgMapping:281,281,0% Bookmark:0,0,0% Changeset:277,277,0% FileContent:12,12,0% FileContentMetadata:351,351,0% HgBonsaiMapping:0,0,0% HgChangeset:281,281,0% HgChangesetViaBonsai:0,0,0% HgFileEnvelope:189,189,0% HgFileNode:0,0,0% HgManifest:444,415,6%* (glob)
