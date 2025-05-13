# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setconfig push.edenapi=true
  $ DISALLOW_NON_PUSHREBASE=1 POPULATE_GIT_MAPPING=1 EMIT_OBSMARKERS=1 BLOB_TYPE="blob_files" default_setup
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  starting Mononoke
  cloning repo in hg client 'repo2'
  $ hg up -q master_bookmark

Push commit
  $ touch file1
  $ hg ci -Aqm commit1 --extra hg-git-rename-source=git --extra convert_revision=1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a
  $ hg push -q -r . --to master_bookmark
  $ D="$(hg whereami)"
  $ echo $D
  c53aa0f6c003ee1977e3a98c66e0fffa8eb91b9a

Push another commit
  $ touch file2
  $ hg ci -Aqm commit2 --extra hg-git-rename-source=git --extra convert_revision=2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b
  $ hg push -q -r . --to master_bookmark

Push another commit that conflicts
  $ touch file3
  $ hg ci -Aqm commit3 --extra hg-git-rename-source=git --extra convert_revision=2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b
  $ hg push -r . --to master_bookmark
  pushing rev dfa0c3f7ce4b to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (c361cbcfe03f, dfa0c3f7ce4b] (1 commit) to remote bookmark master_bookmark
  abort: Server error: invalid request: Pushrebase failed: Conflicting mapping Some(BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(956b4e24cedd3cbffa0273c3750f771302699d4136331995b7ac5a68f8b3a73e)) }) detected while inserting git mappings (tried inserting: [BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(0885106bbedd9bf77c83f034a7e45fe4735d1e9d23a84e5f13935b9766e37cdc)) }])
  [255]

Force-push a commit
  $ hg checkout $D
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ touch file4
  $ hg ci -Aqm commit4 --extra hg-git-rename-source=git --extra convert_revision=4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d
  $ hg push -r . --to master_bookmark --force
  pushing rev 643889d0b7b4 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  moving remote bookmark master_bookmark from c361cbcfe03f to 643889d0b7b4

Check that mappings are populated
  $ get_bonsai_git_mapping
  7AF229C8F6ED15A7C73DF5F9B2C2DE5CB588122E29F176397A3C52E41AB96791|1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A
  956B4E24CEDD3CBFFA0273C3750F771302699D4136331995B7AC5A68F8B3A73E|2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B
  A5E2FBF78579282CC78FCDC94F5AEB9513426AC975DBFB07512912DBC2484C22|4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D
  AA53D24251FF3F54B1B2C29AE02826701B2ABEB0079F1BB13B8434B54CD87675|8131B4F1DA6DF2CAEBE93C581DDD303153B338E5
  E32A1E342CDB1E38E88466B4C1A01AE9F410024017AA21DC0A1C5DA6B3963BF2|E7D82AC745060584C51F27EC0FD9C0FE6CDD4E45
  F8C75E41A0C4D29281DF765F39DE47BCA1DCADFDC55ADA4CCC2F6DF567201658|BE393840A21645C52BBDE7E62BDB7269FC3EBB87
