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

# TODO(T168676855): run import command on integration tests
# $ with_stripped_logs mononoke_x_repo_sync 1 0 initial-import --commit "$C" --version-name "$LATEST_CONFIG_VERSION_NAME" --new-bookmark "$NEW_BOOKMARK_NAME"

# $ clone_and_log_large_repo "$NEW_BOOKMARK_NAME" "$C"
