# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"



Setup configuration
  $ run_common_xrepo_sync_with_gitsubmodules_setup
  $ keep_git_submodules_in_config_version "$LATEST_CONFIG_VERSION_NAME" "$SMALL_REPO_ID"


# Test that if, for some reason, we want to keep the git submodules in the
# future, we're able to do so.
# This will sync the commits and keep the git sub-modules changes, so
# when the large repo is cloned, there'll be a failure because they're not supported
Create commit that modifies git submodule in small repo
  $ testtool_drawdag -R "$SMALL_REPO_NAME" --no-default-files <<EOF
  > A-B-C
  > # modify: A "foo/a.txt" "creating foo directory"
  > # modify: A "bar/b.txt" "creating bar directory"
  > # modify: B "bar/c.txt" "random change"
  > # modify: B "foo/git_submodule" git-submodule "creating git submodule"
  > # copy: C "foo/b.txt" "copying file from bar into foo" B "bar/b.txt"
  > # bookmark: C master
  > EOF
  A=7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36
  B=cd6bd41f62adb809024156682965586754610ac4687b2833317151c239a58b71
  C=ab5bf42dd164f61fa2bcb2de20224d8ffb60f12619bb3692f69d7c171dc1c3be

# TODO(T168676855): run import command on integration tests
# $ with_stripped_logs mononoke_x_repo_sync 1 0 initial-import --commit "$C" --version-name "$LATEST_CONFIG_VERSION_NAME" --new-bookmark "$NEW_BOOKMARK_NAME"

# $ clone_and_log_large_repo "$NEW_BOOKMARK_NAME" "$C"
