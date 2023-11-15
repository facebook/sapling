# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"



Setup configuration
  $ run_common_xrepo_sync_with_gitsubmodules_setup

# In this scenario a git submodule is created and implicitly deletes regular
# files. Because the changes to the git submodule aren't synced, the implicit
# deletes need to be made explicit when the commit is synced to the large repo.
Create commit that modifies git submodule in small repo
  $ testtool_drawdag -R "$SMALL_REPO_NAME" --no-default-files <<EOF
  > A-B
  > # modify: A "foo/a/c" "c"
  > # modify: A "foo/a/d" "d"
  > # modify: A "foo/b/e" "e"
  > # modify: A "bar/f/g" "g"
  > # modify: A "bar/h/i" "i"
  > # modify: B "foo" git-submodule "submodule"
  > # bookmark: B master
  > EOF
  A=85dfabda124636200fe6499b65123179020d32c0ab50818b72a8097dcf9b1880
  B=4ab1f8925a8b6a48eaafb3bb8ce5bfb351bd4301c78d557cd799b721b5a4c6e9

# TODO(T168676855): run import command on integration tests
# $ with_stripped_logs mononoke_x_repo_sync 1 0 initial-import --commit "$B" --version-name "$LATEST_CONFIG_VERSION_NAME" --new-bookmark "$NEW_BOOKMARK_NAME"

# $ clone_and_log_large_repo "$NEW_BOOKMARK_NAME" "$B"
