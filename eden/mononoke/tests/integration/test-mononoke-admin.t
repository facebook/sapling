# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=1 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Everything should already be healed, and out of the queue due to optimisations
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 | strip_glog | sed -re '/^(Could not connect to a replica in)|(Monitoring regions:)|(Discovered regions:)/d'
  Fetched 0 distinct put operations
  All caught up, nothing to do
  Iteration rows processed: * rows, *s; total: * rows, *s (glob)
  The last batch was not full size, waiting...

Check bookmarks
  $ mononoke_admin bookmarks log master_bookmark 2>&1 | grep master_bookmark
  * (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

  $ mononoke_admin bookmarks set another_bookmark 26805aba1e600a82e93661149f2313866a221a7b 2>/dev/null

  $ mononoke_admin bookmarks list --kind publishing 2> /dev/null | sort
  another_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b
  master_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b

  $ mononoke_admin bookmarks delete master_bookmark 2> /dev/null

  $ mononoke_admin bookmarks log master_bookmark 2>&1 --start-time 2days | grep master_bookmark
  * (master_bookmark)  manualmove * (glob)
  * (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

  $ mononoke_admin bookmarks log master_bookmark 2>&1 --start-time 2days --end-time 1day | grep master_bookmark | wc -l
  0

  $ mononoke_admin bookmarks list --kind publishing 2> /dev/null
  another_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b

Check blobstore-fetch, normal
  $ mononoke_admin blobstore-fetch changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: *MultiplexedBlobstore* (glob)
  Some(BlobstoreGetData* (glob)

Check blobstore-fetch, with scrub actions
  $ ls blobstore/1/blobs | wc -l
  30
  $ rm blobstore/1/blobs/*changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd*
  $ ls blobstore/1/blobs | wc -l
  29

  $ mononoke_admin --blobstore-scrub-action=ReportOnly blobstore-fetch changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: *ScrubBlobstore* (glob)
  scrub: blobstore_id BlobstoreId(1) not repaired for repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Some(BlobstoreGetData* (glob)
  $ ls blobstore/1/blobs | wc -l
  29

  $ mononoke_admin --blobstore-scrub-action=Repair blobstore-fetch changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: *ScrubBlobstore* (glob)
  scrub: blobstore_id BlobstoreId(1) repaired for repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Some(BlobstoreGetData* (glob)
  $ ls blobstore/1/blobs | wc -l
  30

  $ mononoke_admin mutable-counters list
  highest-imported-gen-num      =3
  $ mononoke_admin mutable-counters set foo 7
  * Value of foo in 0 set to 7 (glob)
  $ mononoke_admin mutable-counters set bar 9
  * Value of bar in 0 set to 9 (glob)
  $ mononoke_admin mutable-counters list
  bar                           =9
  foo                           =7
  highest-imported-gen-num      =3
  $ mononoke_admin mutable-counters get bar
  Some(9)
  $ mononoke_admin mutable-counters get baz
  None
