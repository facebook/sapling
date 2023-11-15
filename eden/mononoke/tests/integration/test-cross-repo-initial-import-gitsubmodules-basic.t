# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"



Setup configuration
  $ run_common_xrepo_sync_with_gitsubmodules_setup

# Basic scenario, where a commit in the small repo contains changes to ordinary 
# files and to a git submodule. The latter changes should be dropped when 
# syncing any commit to the large repo.
Create commit that modifies git submodule in small repo
  $ testtool_drawdag -R "$SMALL_REPO_NAME" --no-default-files <<EOF
  > A-B-C
  > # modify: A "foo/a.txt" "creating foo directory"
  > # modify: A "bar/b.txt" "creating bar directory"
  > # modify: A "foo/git_submodule" git-submodule "creating git submodule"
  > # modify: B "bar/c.txt" "random change"
  > # copy: B "bar/git_submodule" git-submodule "another git submodule" A "foo/git_submodule"
  > # copy: C "foo/b.txt" "copying file from bar into foo" B "bar/b.txt"
  > # modify: C "foo/git_submodule" git-submodule "modify git submodule"
  > # modify: C "bar/git_submodule" git-submodule "modify git submodule"
  > # bookmark: C master
  > EOF
  A=fc6effdc9aa8985d2242af2849c01ff44c02283707b03c8f9dee9c0ee0ab10fd
  B=25e96de5ca32ae6a1c791fda2a352793d43e2c0da23ddf5cd6756b00670b4a00
  C=00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821

  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID"  "$LARGE_REPO_ID" initial-import --commit "$C" --version-name "$LATEST_CONFIG_VERSION_NAME" --new-bookmark "$NEW_BOOKMARK_NAME"
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  changeset resolved as: ChangesetId(Blake2(00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821))
  Checking if 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821 is already synced 1->0
  syncing 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821
  Setting bookmark SYNCED_HEAD to changeset 768a9c6d2b6943900f9d4374028a891c7d3dc62d7ecc25a1fd2a9c3fc9aba14b
  changeset 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821 synced as 768a9c6d2b6943900f9d4374028a891c7d3dc62d7ecc25a1fd2a9c3fc9aba14b in * (glob)
  successful sync


  $ clone_and_log_large_repo "$NEW_BOOKMARK_NAME" "$C"
  commit:      d6af9545397f
  bookmark:    SYNCED_HEAD
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     C
  
   smallrepofolder1/foo/b.txt |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  commit:      7e73c01eea00
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B
  
   smallrepofolder1/bar/c.txt |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  commit:      e462fc947f26
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A
  
   smallrepofolder1/bar/b.txt |  1 +
   smallrepofolder1/foo/a.txt |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  changeset resolved as: ChangesetId(Blake2(00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821))
  RewrittenAs([(ChangesetId(Blake2(768a9c6d2b6943900f9d4374028a891c7d3dc62d7ecc25a1fd2a9c3fc9aba14b)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
