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
  D=9a80ec17c5caf2e67b0ae8df26d7a6f111ea5cc3ddc2077731fa2711cf457ea7
  E=54f6eed1de6b6caeb23c17044f9bf8133aa20e68b1b9cf7057f1eb7fe5b48a73

  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID"  "$LARGE_REPO_ID" initial-import --no-progress-bar --version-name "$LATEST_CONFIG_VERSION_NAME" --all-bookmarks
  Starting session with id * (glob)
  Checking if 54f6eed1de6b6caeb23c17044f9bf8133aa20e68b1b9cf7057f1eb7fe5b48a73 is already synced 1->0
  syncing 54f6eed1de6b6caeb23c17044f9bf8133aa20e68b1b9cf7057f1eb7fe5b48a73
  Found 3 unsynced ancestors
  changeset 54f6eed1de6b6caeb23c17044f9bf8133aa20e68b1b9cf7057f1eb7fe5b48a73 synced as 8443fe4b35302f08abf9ffa53d494b7175d7514c04e4cd52fc78f8369cbd5a83 in *ms (glob)
  successful sync of head 54f6eed1de6b6caeb23c17044f9bf8133aa20e68b1b9cf7057f1eb7fe5b48a73
  Checking if 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821 is already synced 1->0
  syncing 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821
  Found 2 unsynced ancestors
  changeset 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821 synced as 768a9c6d2b6943900f9d4374028a891c7d3dc62d7ecc25a1fd2a9c3fc9aba14b in * (glob)
  successful sync of head 00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821


  $ clone_and_log_large_repo 768a9c6d2b6943900f9d4374028a891c7d3dc62d7ecc25a1fd2a9c3fc9aba14b 8443fe4b35302f08abf9ffa53d494b7175d7514c04e4cd52fc78f8369cbd5a83
  o  1c47bac19fb8 E
  │
  o  df0e7f5dd366 D
  │
  │ o  d6af9545397f C
  │ │   smallrepofolder1/foo/b.txt |  1 +
  │ │   1 files changed, 1 insertions(+), 0 deletions(-)
  │ │
  │ o  7e73c01eea00 B
  ├─╯   smallrepofolder1/bar/c.txt |  1 +
  │     1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  e462fc947f26 A
      smallrepofolder1/bar/b.txt |  1 +
      smallrepofolder1/foo/a.txt |  1 +
      2 files changed, 2 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(00b0b4d6130a22ccf3fada118572a85a6bb2d7c253d4285557802b7b8f250821)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  RewrittenAs([(ChangesetId(Blake2(54f6eed1de6b6caeb23c17044f9bf8133aa20e68b1b9cf7057f1eb7fe5b48a73)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types
