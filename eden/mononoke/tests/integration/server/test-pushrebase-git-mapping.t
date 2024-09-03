# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 DISALLOW_NON_PUSHREBASE=1 POPULATE_GIT_MAPPING=1 EMIT_OBSMARKERS=1 BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'
  $ hg up -q master_bookmark
  $ C="$(hg whereami)"

Push commit
  $ touch file1
  $ hg ci -Aqm commit1 --extra hg-git-rename-source=git --extra convert_revision=1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a
  $ hg push -q -r . --to master_bookmark
  $ wait_for_bookmark_move_away_edenapi repo master_bookmark "$C"
  $ D="$(hg whereami)"

Push another commit
  $ touch file2
  $ hg ci -Aqm commit2 --extra hg-git-rename-source=git --extra convert_revision=2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b
  $ hg push -q -r . --to master_bookmark
  $ wait_for_bookmark_move_away_edenapi repo master_bookmark "$D"

Push another commit that conflicts
  $ touch file3
  $ hg ci -Aqm commit3 --extra hg-git-rename-source=git --extra convert_revision=2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b
  $ hg push -r . --to master_bookmark
  pushing rev 7fc28fc53a18 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (9dde85abe808, 7fc28fc53a18] (1 commit) to remote bookmark master_bookmark
  abort: Server error: invalid request: Pushrebase failed: Conflicting mapping Some(BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(e37e13b17b5c2b37965b2a9591a64cb2c44a68fd10f1362a595da8c6e4eefa41)) }) detected while inserting git mappings (tried inserting: [BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(3fa7acdeb82ac4f96a7bf1e7b5fa8f661c9921954a46164cbbfa828c0485595b)) }])
  [255]

Force-push a commit
  $ hg prev 2
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  [2388bc] commit1
  $ touch file4
  $ hg ci -Aqm commit4 --extra hg-git-rename-source=git --extra convert_revision=4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d
  $ hg push -r . --to master_bookmark --force
  pushing rev 1b5b68e81ae5 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  moving remote bookmark master_bookmark from 9dde85abe808 to 1b5b68e81ae5

Check that mappings are populated
  $ get_bonsai_git_mapping
  3CEE0520D115C5973E538AFDEB6985C1DF2CFC2C8E58CE465B855D73993EFBA1|1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A
  E37E13B17B5C2B37965B2A9591A64CB2C44A68FD10F1362A595DA8C6E4EEFA41|2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B
  32C125F232EF84EAD04050D1B0245B26EFFD4A8FF40292A54401A0AE40B1A63F|4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D
