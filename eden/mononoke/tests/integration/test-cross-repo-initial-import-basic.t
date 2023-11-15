# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"



Setup configuration
  $ run_common_xrepo_sync_with_gitsubmodules_setup

# Simple integration test for the initial-import command in the forward syncer
Create small repo commits
  $ testtool_drawdag -R "$SMALL_REPO_NAME" --no-default-files <<EOF
  > A-B-C
  > # modify: A "foo/a.txt" "creating foo directory"
  > # modify: A "bar/b.txt" "creating bar directory"
  > # modify: B "bar/c.txt" "random change"
  > # modify: B "foo/d" "another random change"
  > # copy: C "foo/b.txt" "copying file from bar into foo" B "bar/b.txt"
  > # bookmark: C master
  > EOF
  A=7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36
  B=2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3
  C=738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7


# TODO(T168676855): run import command on integration tests
# $ with_stripped_logs mononoke_x_repo_sync 1 0 initial-import --commit "$C" --version-name "$LATEST_CONFIG_VERSION_NAME" --new-bookmark "$NEW_BOOKMARK_NAME"

# $ clone_and_log_large_repo "$NEW_BOOKMARK_NAME" "$C"
