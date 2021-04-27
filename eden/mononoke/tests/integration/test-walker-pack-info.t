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
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Run a scrub with the pack logging enabled
  $ mononoke_walker -l loaded scrub -q -I deep -i bonsai -i FileContent -b master_bookmark -a all --pack-log-scuba-file pack-info.json 2>&1 | strip_glog
  Seen,Loaded: 7,7

Check logged pack info
  $ LINES="$(wc -l < pack-info.json)"
  $ [[ $LINES -lt 50 ]]
  $ jq -r '.int * .normal | [ .chunk_num, .blobstore_key, .node_type, .node_fingerprint, .similarity_key, .uncompressed_size ] | @csv' < pack-info.json | sort | uniq
  1,"repo0000.changeset.blake2.459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66","Changeset",2040214566370451200,,104
  1,"repo0000.changeset.blake2.9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec","Changeset",-3468459737349231600,,69
  1,"repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd","Changeset",-975483755298211600,,104
  1,"repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","FileContent",-5148279705570089000,,4
  1,"repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","FileContent",4679342931123203000,,4
  1,"repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","FileContent",-771035176585636100,,4
