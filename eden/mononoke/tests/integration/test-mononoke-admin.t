# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=1 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Drain the healer queue
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 | strip_glog | sed -re '/^(Could not connect to a replica in)|(Monitoring regions:)|(Discovered regions:)/d'
  Replication lag is * (glob)
  Fetched 60 queue entires (before building healing futures)
  Out of them 30 distinct blobstore keys, 30 distinct operation keys
  Found 30 blobs to be healed... Doing it
  For 30 blobs did HealStats { queue_add: 0, queue_del: 60, put_success: 0, put_failure: 0 }
  Deleting 60 actioned queue entries
  The last batch was not full size, waiting...

Check bookmarks
  $ mononoke_admin bookmarks log master_bookmark 2>&1 | grep master_bookmark
  (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

  $ mononoke_admin bookmarks set another_bookmark 26805aba1e600a82e93661149f2313866a221a7b 2>/dev/null

  $ mononoke_admin bookmarks list --kind publishing 2> /dev/null | sort
  another_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b
  master_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b

  $ mononoke_admin bookmarks delete master_bookmark 2> /dev/null

  $ mononoke_admin bookmarks list --kind publishing 2> /dev/null
  another_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b

Check blobstore-fetch, normal
  $ mononoke_admin blobstore-fetch changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: *MultiplexedBlobstore* (glob)
  Some(BlobstoreGetData* (glob)

Check blobstore-fetch, with scrub actions
  $ ls blobstore/1/blobs | count_stdin_lines
  30
  $ rm blobstore/1/blobs/*changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd*
  $ ls blobstore/1/blobs | count_stdin_lines
  29

  $ mononoke_admin blobstore-fetch --scrub-blobstore-action=ReportOnly changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: *ScrubBlobstore* (glob)
  scrub: blobstore_id BlobstoreId(1) not repaired for repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Some(BlobstoreGetData* (glob)
  $ ls blobstore/1/blobs | count_stdin_lines
  29

  $ mononoke_admin blobstore-fetch --scrub-blobstore-action=Repair changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: *ScrubBlobstore* (glob)
  scrub: blobstore_id BlobstoreId(1) repaired for repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Some(BlobstoreGetData* (glob)
  $ ls blobstore/1/blobs | count_stdin_lines
  30

  $ mononoke_admin mutable-counters list
  $ mononoke_admin mutable-counters set foo 7
  * Value of foo in 0 set to 7 (glob)
  $ mononoke_admin mutable-counters set bar 9
  * Value of bar in 0 set to 9 (glob)
  $ mononoke_admin mutable-counters list
  bar                           =9
  foo                           =7
  $ mononoke_admin mutable-counters get bar
  Some(9)
  $ mononoke_admin mutable-counters get baz
  None

Check filestore store & fetch

  $ echo foo > "$TESTTMP/blob"

  $ mononoke_admin filestore store "$TESTTMP/blob"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Wrote 2ff003c268263a870defffe9afdccd3a72e501bbd892f24cac7ca944ac240eb1 (4 bytes) (glob)

  $ mononoke_admin filestore fetch id 2ff003c268263a870defffe9afdccd3a72e501bbd892f24cac7ca944ac240eb1 2>/dev/null
  foo
