# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-snapshot.sh"

setup configuration
  $ base_snapshot_repo_setup client1
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

Fetch info about a bubble that exists:
  $ mononoke_newadmin ephemeral-store -R repo info -b 1
  BubbleID: 1
  ChangesetIDs: [ChangesetId(Blake2(60ba9e25af931d7b1669e121cb4f42ad0eeca14462e8e8126140ca63a25bee8e))]
  RepoID: 0
  ExpiryDate: *+00:00 (glob)
  Status: Active
  BlobstorePrefix: eph1.

Fetch info about a bubble that doesn't exist:
  $ mononoke_newadmin ephemeral-store -R repo info -b 100001
  Error: bubble 100001 does not exist, or has expired
  [1]

Fetch info about a bubble based on a valid changeset ID:
  $ mononoke_newadmin ephemeral-store -R repo info -i 41b1e99e2b81202d04b4817e3fa7ebdb936184626f74af23b865a80fa71b5561
  BubbleID: 2
  ChangesetIDs: [ChangesetId(Blake2(41b1e99e2b81202d04b4817e3fa7ebdb936184626f74af23b865a80fa71b5561))]
  RepoID: 0
  ExpiryDate: *+00:00 (glob)
  Status: Active
  BlobstorePrefix: eph2.

Fetch info about a bubble based on an invalid changeset ID:
  $ mononoke_newadmin ephemeral-store -R repo info -i ofcourse_this_is_invalid
  Error: invalid blake2 input: need exactly 64 hex digits
  [1]
Fetch info about a bubble based on a non-matching changeset ID:
  $ mononoke_newadmin ephemeral-store -R repo info -i 49c49a9ad363e4a2f0c314093683a84a85bfaa7b4da83046e58ccb4fbeb2f6c5
  Error: No bubble exists for changeset ID 49c49a9ad363e4a2f0c314093683a84a85bfaa7b4da83046e58ccb4fbeb2f6c5
  [1]
