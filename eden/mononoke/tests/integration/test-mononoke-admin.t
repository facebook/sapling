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

Check blobstore-fetch, normal
  $ mononoke_admin blobstore-fetch changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: *MultiplexedBlobstore* (glob)
  Some(BlobstoreGetData* (glob)

Check blobstore-fetch, with scrub actions
  $ ls blobstore/1/blobs | wc -l
  33
  $ rm blobstore/1/blobs/*changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd*
  $ ls blobstore/1/blobs | wc -l
  32

  $ mononoke_admin --blobstore-scrub-action=ReportOnly blobstore-fetch changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: *ScrubBlobstore* (glob)
  scrub: blobstore_id BlobstoreId(1) not repaired for repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Some(BlobstoreGetData* (glob)
  $ ls blobstore/1/blobs | wc -l
  32

  $ mononoke_admin --blobstore-scrub-action=Repair blobstore-fetch changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: *ScrubBlobstore* (glob)
  scrub: blobstore_id BlobstoreId(1) repaired for repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Some(BlobstoreGetData* (glob)
  $ ls blobstore/1/blobs | wc -l
  33
