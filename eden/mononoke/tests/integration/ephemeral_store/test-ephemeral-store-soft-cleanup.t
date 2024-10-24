# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-snapshot.sh"

setup configuration
# Need short-term expiring bubbles with mark-and-delete deletion mode
  $ BUBBLE_DELETION_MODE=1 BUBBLE_EXPIRATION_SECS=0 BUBBLE_LIFESPAN_SECS=10 base_snapshot_repo_setup client1
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  $ cd client1
  $ mkdir test_tmp
  $ cd test_tmp
  $ echo "a file content" > a
  $ hg snapshot create
  snapshot: Snapshot created with id 60ba9e25af931d7b1669e121cb4f42ad0eeca14462e8e8126140ca63a25bee8e
  $ echo "b file content" > b
  $ hg add b
  $ hg snapshot create
  snapshot: Snapshot created with id 41b1e99e2b81202d04b4817e3fa7ebdb936184626f74af23b865a80fa71b5561
  $ echo "c file content" > c
  $ hg snapshot create
  snapshot: Snapshot created with id 2a2db020a9a64a3541d655f0b8a14c4df3f26ce584d5e1945da2b5ef4aefe43c
  $ echo "d file content" > d
  $ hg snapshot create
  snapshot: Snapshot created with id 0458064f09cc3f249dcdef56198bd70181ceae750a7512b842af8411dbcdb97d
# Ensure bubbles are expired before moving forward
  $ sleep 10

Cleanup expired bubbles in dry-run mode:
  $ mononoke_newadmin ephemeral-store -R repo cleanup -n -c 0
  Fetched 4 expired bubbles for deletion
  Executing cleanup in dry-run mode. The following bubbles were fetched for deletion:
  [BubbleId(1), BubbleId(2), BubbleId(3), BubbleId(4)]

Cleanup expired bubbles with too high cut-off
  $ mononoke_newadmin ephemeral-store -R repo cleanup -n -c 100000
  No expired bubbles found for deletion based on input provided

Cleanup expired bubbles with zero as the limit
  $ mononoke_newadmin ephemeral-store -R repo cleanup -l 0
  No expired bubbles found for deletion based on input provided

Cleanup expired bubbles in dry-run mode with non-zero as the limit
  $ mononoke_newadmin ephemeral-store -R repo cleanup -l 2 -n -c 0
  Fetched 2 expired bubbles for deletion
  Executing cleanup in dry-run mode. The following bubbles were fetched for deletion:
  [BubbleId(1), BubbleId(2)]

Before doing actual clean-up, verify that the blob indeed exists
  $ cd ../../
  $ [ -f "blobstore/blobs/blob-eph1.repo0000.changeset.blake2.60ba9e25af931d7b1669e121cb4f42ad0eeca14462e8e8126140ca63a25bee8e" ] && echo "Blob still present"
  Blob still present

Cleanup expired bubbles with no cut-off
  $ mononoke_newadmin ephemeral-store -R repo cleanup -c 0
  Fetched 4 expired bubbles for deletion
  Cleaned up bubble 1 and deleted 0 blob keys contained in it
  Cleaned up bubble 2 and deleted 0 blob keys contained in it
  Cleaned up bubble 3 and deleted 0 blob keys contained in it
  Cleaned up bubble 4 and deleted 0 blob keys contained in it

# Since deletion_mode=MARK_ONLY, the blob keys within the bubbles
# should NOT be deleted
Verify the blob keys are NOT actually deleted (using a key from Bubble 1).
  $ [ -f "blobstore/blobs/blob-eph1.repo0000.changeset.blake2.60ba9e25af931d7b1669e121cb4f42ad0eeca14462e8e8126140ca63a25bee8e" ] && echo "Blob still present"
  Blob still present
