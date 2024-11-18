# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

-- Define the large and small repo ids and names before calling any helpers
  $ export LARGE_REPO_NAME="large_repo"
  $ export LARGE_REPO_ID=10
  $ export SUBMODULE_REPO_NAME="small_repo"
  $ export SUBMODULE_REPO_ID=11

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"



Setup configuration
  $ run_common_xrepo_sync_with_gitsubmodules_setup
  L_A=b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4

# Basic scenario, where a commit in the small repo contains changes to ordinary
# files and to a git submodule. The latter changes should be dropped when
# syncing any commit to the large repo.
Create commit that modifies git submodule in small repo
  $ testtool_drawdag -R "$SUBMODULE_REPO_NAME" --no-default-files <<EOF
  > A-B-C
  >  \
  >   D-E
  > # modify: A "foo/a.txt" "creating foo directory"
  > # modify: A "bar/b.txt" "creating bar directory"
  > # modify: A "foo/git_submodule" git-submodule "creating git submodule"
  > # modify: B "bar/c.txt" "random change"
  > # copy: B "bar/git_submodule" git-submodule "another git submodule" A "foo/git_submodule"
  > # copy: C "foo/b.txt" "copying file from bar into foo" B "bar/b.txt"
  > # modify: C "foo/git_submodule" git-submodule "modify git submodule"
  > # modify: C "bar/git_submodule" git-submodule "modify git submodule"
  > # bookmark: C master
  > # bookmark: E feature
  > EOF
  A=fc6effdc9aa8985d2242af2849c01ff44c02283707b03c8f9dee9c0ee0ab10fd
  B=25e96de5ca32ae6a1c791fda2a352793d43e2c0da23ddf5cd6756b00670b4a00
  C=00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821
  D=a3d81f95df47e4c8a25e2ea1a171bc186af856a8072b2c0490b7c03ffb5a5680
  E=1d0633cca456dfccb06ca24646024da1a8f42204f91e54d765fc4e5f2ad87bbe

  $ with_stripped_logs mononoke_x_repo_sync "$SUBMODULE_REPO_ID" "$LARGE_REPO_ID" \
  > initial-import --no-progress-bar --version-name "$LATEST_CONFIG_VERSION_NAME" \
  > --all-bookmarks | tee $TESTTMP/initial_import.out
  Starting session with id * (glob)
  Starting up X Repo Sync from small repo small_repo to large repo large_repo
  Checking if 1d0633cca456dfccb06ca24646024da1a8f42204f91e54d765fc4e5f2ad87bbe is already synced 11->10
  Syncing 1d0633cca456dfccb06ca24646024da1a8f42204f91e54d765fc4e5f2ad87bbe for inital import
  Source repo: small_repo / Target repo: large_repo
  Found 2 unsynced ancestors
  changeset 1d0633cca456dfccb06ca24646024da1a8f42204f91e54d765fc4e5f2ad87bbe synced as 1d0633cca456dfccb06ca24646024da1a8f42204f91e54d765fc4e5f2ad87bbe * (glob)
  successful sync of head 1d0633cca456dfccb06ca24646024da1a8f42204f91e54d765fc4e5f2ad87bbe
  Checking if 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821 is already synced 11->10
  Syncing 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821 for inital import
  Source repo: small_repo / Target repo: large_repo
  Found 3 unsynced ancestors
  changeset 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821 synced as 768a9c6d2b6943900f9d4374028a891c7d3dc62d7ecc25a1fd2a9c3fc9aba14b in * (glob)
  successful sync of head 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821
  X Repo Sync execution finished from small repo small_repo to large repo large_repo


  $ SYNCED_HEAD=$(rg ".+synced as (\w+) .+" -or '$1' "$TESTTMP/initial_import.out" | tail -n1)
  $ clone_and_log_large_repo "$SYNCED_HEAD" 1d0633cca456dfccb06ca24646024da1a8f42204f91e54d765fc4e5f2ad87bbe
  o  b05932cd5d83 E
  │
  o  f1188a9d73dd D
  
  o  d6af9545397f C
  │   smallrepofolder1/foo/b.txt |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  7e73c01eea00 B
  │   smallrepofolder1/bar/c.txt |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  e462fc947f26 A
      smallrepofolder1/bar/b.txt |  1 +
      smallrepofolder1/foo/a.txt |  1 +
      2 files changed, 2 insertions(+), 0 deletions(-)
  
  @  54a6db91baf1 L_A
      file_in_large_repo.txt |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  RewrittenAs([(ChangesetId(Blake2(1d0633cca456dfccb06ca24646024da1a8f42204f91e54d765fc4e5f2ad87bbe)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types














  $ REPOIDLARGE=$LARGE_REPO_ID REPOIDSMALL=$SUBMODULE_REPO_ID verify_wc 768a9c6d2b6943900f9d4374028a891c7d3dc62d7ecc25a1fd2a9c3fc9aba14b
