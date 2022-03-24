# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-snapshot.sh"

setup configuration
  $ base_snapshot_repo_setup client1
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

Fetch info about a bubble that exists:
  $ mononoke_newadmin ephemeral-store -R repo info -b 1
  BubbleID: 1
  ChangesetIDs: [ChangesetId(Blake2(39c49a9ad363e4a2f0c314093683a84a85bfaa7b4da83046e58ccb4fbeb2f6c5))]
  RepoID: 0
  ExpiryDate: *+00:00 (glob)
  Status: Active
  BlobstorePrefix: eph1.

Fetch info about a bubble that doesn't exist:
  $ mononoke_newadmin ephemeral-store -R repo info -b 100001
  Error: bubble 100001 does not exist, or has expired
  [1]

Fetch info about a bubble based on a valid changeset ID:
  $ mononoke_newadmin ephemeral-store -R repo info -i a70032dd92c595f7c63727c331bff544b49b93655f5df698c756de0ca6e707be
  BubbleID: 2
  ChangesetIDs: [ChangesetId(Blake2(a70032dd92c595f7c63727c331bff544b49b93655f5df698c756de0ca6e707be))]
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
