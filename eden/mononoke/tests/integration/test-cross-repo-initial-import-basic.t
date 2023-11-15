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


  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID"  "$LARGE_REPO_ID" initial-import --commit "$C" --version-name "$LATEST_CONFIG_VERSION_NAME" --new-bookmark "$NEW_BOOKMARK_NAME"
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  changeset resolved as: ChangesetId(Blake2(738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7))
  Checking if 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7 is already synced 1->0
  syncing 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7
  Setting bookmark SYNCED_HEAD to changeset ca175120dfe7fb7fcb0d872e26ce331cb24c7d9ec457d599a40684527c65d63a
  changeset 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7 synced as ca175120dfe7fb7fcb0d872e26ce331cb24c7d9ec457d599a40684527c65d63a in * (glob)
  successful sync

  $ clone_and_log_large_repo "$NEW_BOOKMARK_NAME" "$C"
  commit:      cbb9c8a988b5
  bookmark:    SYNCED_HEAD
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     C
  
   smallrepofolder1/foo/b.txt |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  commit:      5e3f6798b6a3
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B
  
   smallrepofolder1/bar/c.txt |  1 +
   smallrepofolder1/foo/d     |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)
  
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
  changeset resolved as: ChangesetId(Blake2(738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7))
  RewrittenAs([(ChangesetId(Blake2(ca175120dfe7fb7fcb0d872e26ce331cb24c7d9ec457d599a40684527c65d63a)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
