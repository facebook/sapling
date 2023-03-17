# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-snapshot.sh"

setup configuration
# Need short-term expiring bubbles with mark-and-delete deletion mode
  $ BUBBLE_DELETION_MODE=2 BUBBLE_EXPIRATION_SECS=0 BUBBLE_LIFESPAN_SECS=10 base_snapshot_repo_setup client1
  $ cd repo  
  $ mkdir test_tmp
  $ cd test_tmp
  $ echo "a file content" > a
  $ hgedenapi snapshot create
  snapshot: Snapshot created with id 39c49a9ad363e4a2f0c314093683a84a85bfaa7b4da83046e58ccb4fbeb2f6c5
  $ echo "b file content" > b
  $ hg add b
  $ hgedenapi snapshot create
  snapshot: Snapshot created with id a70032dd92c595f7c63727c331bff544b49b93655f5df698c756de0ca6e707be
  $ echo "c file content" > c
  $ hgedenapi snapshot create
  snapshot: Snapshot created with id 29bc19b1061371d50be8982b75d12495f5c9f7dc26c1cbf6edccf225e0af6712
  $ echo "d file content" > d
  $ echo "e file content" > e
  $ echo "f file content" > f
  $ echo "g file content" > g
  $ hg add g
  $ hgedenapi snapshot create
  snapshot: Snapshot created with id b4137f355ae75d51de5b7688312fb6dcd6791ed7d65e2e6ec5bc605d86a1afcb
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
  $ [ -f "blobstore/blobs/blob-eph1.repo0000.changeset.blake2.39c49a9ad363e4a2f0c314093683a84a85bfaa7b4da83046e58ccb4fbeb2f6c5" ] && echo "Blob still present"
  Blob still present

Cleanup expired bubbles with no cut-off
  $ mononoke_newadmin ephemeral-store -R repo cleanup -c 0
  Fetched 4 expired bubbles for deletion
  Cleaned up bubble 1 and deleted 6 blob keys contained in it
  Cleaned up bubble 2 and deleted 11 blob keys contained in it
  Cleaned up bubble 3 and deleted 16 blob keys contained in it
  Cleaned up bubble 4 and deleted 36 blob keys contained in it

# Since deletion_mode=MARK_AND_DELETE, the blob keys within the bubbles
# need to be actually deleted.
Verify the blob keys are indeed deleted (using a key from Bubble 1). Below query should NOT return the matching blob.
  $ [ -f "blobstore/blobs/blob-eph1.repo0000.changeset.blake2.39c49a9ad363e4a2f0c314093683a84a85bfaa7b4da83046e58ccb4fbeb2f6c5" ] && echo "Blob still present"
  [1]
# No output indicating the blob file is not present  
