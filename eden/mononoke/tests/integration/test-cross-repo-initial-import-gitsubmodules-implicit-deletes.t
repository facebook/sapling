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

  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID"  "$LARGE_REPO_ID" initial-import --no-progress-bar --version-name "$LATEST_CONFIG_VERSION_NAME" --all-bookmarks
  Starting session with id * (glob)
  Checking if 4ab1f8925a8b6a48eaafb3bb8ce5bfb351bd4301c78d557cd799b721b5a4c6e9 is already synced 1->0
  syncing 4ab1f8925a8b6a48eaafb3bb8ce5bfb351bd4301c78d557cd799b721b5a4c6e9
  Found 2 unsynced ancestors
  changeset 4ab1f8925a8b6a48eaafb3bb8ce5bfb351bd4301c78d557cd799b721b5a4c6e9 synced as 30a912050a27826f649bcce7bd1b2fbfe1bf9b2883dcabc17753c2f9f1ab3ad5 in * (glob)
  successful sync of head 4ab1f8925a8b6a48eaafb3bb8ce5bfb351bd4301c78d557cd799b721b5a4c6e9



  $ clone_and_log_large_repo "30a912050a27826f649bcce7bd1b2fbfe1bf9b2883dcabc17753c2f9f1ab3ad5"
  o  9b75b98ff186 B
  │   smallrepofolder1/foo/a/c |  1 -
  │   smallrepofolder1/foo/a/d |  1 -
  │   smallrepofolder1/foo/b/e |  1 -
  │   3 files changed, 0 insertions(+), 3 deletions(-)
  │
  o  22cdfd416dbb A
      smallrepofolder1/bar/f/g |  1 +
      smallrepofolder1/bar/h/i |  1 +
      smallrepofolder1/foo/a/c |  1 +
      smallrepofolder1/foo/a/d |  1 +
      smallrepofolder1/foo/b/e |  1 +
      5 files changed, 5 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(4ab1f8925a8b6a48eaafb3bb8ce5bfb351bd4301c78d557cd799b721b5a4c6e9)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types
