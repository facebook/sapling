# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ DISALLOW_NON_PUSHREBASE=1 POPULATE_GIT_MAPPING=1 EMIT_OBSMARKERS=1 BLOB_TYPE="blob_files" default_setup
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

Push commit
  $ touch file1
  $ hg ci -Aqm commit1 --extra hg-git-rename-source=git --extra convert_revision=1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a
  $ hgmn push -q -r . --to master_bookmark

Push another commit
  $ touch file2
  $ hg ci -Aqm commit2 --extra hg-git-rename-source=git --extra convert_revision=2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b
  $ hgmn push -q -r . --to master_bookmark

Push another commit that conflicts
  $ touch file3
  $ hg ci -Aqm commit3 --extra hg-git-rename-source=git --extra convert_revision=2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Pushrebase failed: Conflicting mapping Some(BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(e37e13b17b5c2b37965b2a9591a64cb2c44a68fd10f1362a595da8c6e4eefa41)) }) detected while inserting git mappings (tried inserting: [BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(3fa7acdeb82ac4f96a7bf1e7b5fa8f661c9921954a46164cbbfa828c0485595b)) }])
  remote: 
  remote:   Root cause:
  remote:     Conflicting mapping Some(BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(e37e13b17b5c2b37965b2a9591a64cb2c44a68fd10f1362a595da8c6e4eefa41)) }) detected while inserting git mappings (tried inserting: [BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(3fa7acdeb82ac4f96a7bf1e7b5fa8f661c9921954a46164cbbfa828c0485595b)) }])
  remote: 
  remote:   Caused by:
  remote:     Conflicting mapping Some(BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(e37e13b17b5c2b37965b2a9591a64cb2c44a68fd10f1362a595da8c6e4eefa41)) }) detected while inserting git mappings (tried inserting: [BonsaiGitMappingEntry { git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b), bcs_id: ChangesetId(Blake2(3fa7acdeb82ac4f96a7bf1e7b5fa8f661c9921954a46164cbbfa828c0485595b)) }])
  remote: 
  remote:   Debug context:
  remote:     PushrebaseError(
  remote:         Error(
  remote:             Conflict(
  remote:                 Some(
  remote:                     BonsaiGitMappingEntry {
  remote:                         git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b),
  remote:                         bcs_id: ChangesetId(
  remote:                             Blake2(e37e13b17b5c2b37965b2a9591a64cb2c44a68fd10f1362a595da8c6e4eefa41),
  remote:                         ),
  remote:                     },
  remote:                 ),
  remote:                 [
  remote:                     BonsaiGitMappingEntry {
  remote:                         git_sha1: GitSha1(2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b),
  remote:                         bcs_id: ChangesetId(
  remote:                             Blake2(3fa7acdeb82ac4f96a7bf1e7b5fa8f661c9921954a46164cbbfa828c0485595b),
  remote:                         ),
  remote:                     },
  remote:                 ],
  remote:             ),
  remote:         ),
  remote:     )
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Force-push a commit
  $ hg prev 2
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  [2388bc] commit1
  $ touch file4
  $ hg ci -Aqm commit4 --extra hg-git-rename-source=git --extra convert_revision=4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d
  $ hgmn push -r . --to master_bookmark --force
  pushing rev 1b5b68e81ae5 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Check that mappings are populated
  $ get_bonsai_git_mapping
  3CEE0520D115C5973E538AFDEB6985C1DF2CFC2C8E58CE465B855D73993EFBA1|1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A
  E37E13B17B5C2B37965B2A9591A64CB2C44A68FD10F1362A595DA8C6E4EEFA41|2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B
  32C125F232EF84EAD04050D1B0245B26EFFD4A8FF40292A54401A0AE40B1A63F|4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D
