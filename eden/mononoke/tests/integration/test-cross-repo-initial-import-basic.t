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


  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID"  "$LARGE_REPO_ID" initial-import --no-progress-bar -i "$C" --version-name "$LATEST_CONFIG_VERSION_NAME"
  Starting session with id * (glob)
  Checking if 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7 is already synced 1->0
  syncing 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7
  Found 3 unsynced ancestors
  changeset 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7 synced as ca175120dfe7fb7fcb0d872e26ce331cb24c7d9ec457d599a40684527c65d63a in * (glob)
  successful sync of head 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7

  $ clone_and_log_large_repo "ca175120dfe7fb7fcb0d872e26ce331cb24c7d9ec457d599a40684527c65d63a"
  o  cbb9c8a988b5 C
  │   smallrepofolder1/foo/b.txt |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  5e3f6798b6a3 B
  │   smallrepofolder1/bar/c.txt |  1 +
  │   smallrepofolder1/foo/d     |  1 +
  │   2 files changed, 2 insertions(+), 0 deletions(-)
  │
  o  e462fc947f26 A
      smallrepofolder1/bar/b.txt |  1 +
      smallrepofolder1/foo/a.txt |  1 +
      2 files changed, 2 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types
