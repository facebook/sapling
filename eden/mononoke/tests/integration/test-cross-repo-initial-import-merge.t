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
  > A-B-C-M
  >  \   /
  >   D-E
  > # modify: A "foo/a.txt" "creating foo directory"
  > # modify: A "bar/b.txt" "creating bar directory"
  > # modify: B "bar/c.txt" "random change"
  > # modify: B "foo/d" "another random change"
  > # copy: C "foo/b.txt" "copying file from bar into foo" B "bar/b.txt"
  > # modify: E "foo/e" "File E"
  > # bookmark: M master
  > EOF
  A=7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36
  B=2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3
  C=738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7
  D=7116ef2595ff4ce61ab27e3148a35960d96a969a833ec8e7225a083d2f3b3187
  E=e774907679bfb4c154130656b2c8842c192eeffd3de6b6c7fdafd0973522e43a
  M=3eb23b278c44bf5d812c96f2a3211408d2a779b566984670127eebcd01fe459d


  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID"  "$LARGE_REPO_ID" initial-import --no-progress-bar -i "$M" --version-name "$LATEST_CONFIG_VERSION_NAME"
  Starting session with id * (glob)
  Checking if 3eb23b278c44bf5d812c96f2a3211408d2a779b566984670127eebcd01fe459d is already synced 1->0
  syncing 3eb23b278c44bf5d812c96f2a3211408d2a779b566984670127eebcd01fe459d
  Found 6 unsynced ancestors
  changeset 3eb23b278c44bf5d812c96f2a3211408d2a779b566984670127eebcd01fe459d synced as 154e057495ead9af16d2ad3401b1fca7a7d23e39a295e277d84ba37f244e48ff in *ms (glob)
  successful sync of head 3eb23b278c44bf5d812c96f2a3211408d2a779b566984670127eebcd01fe459d


  $ clone_and_log_large_repo 154e057495ead9af16d2ad3401b1fca7a7d23e39a295e277d84ba37f244e48ff
  o    fb2024205bd9 M
  ├─╮   smallrepofolder1/foo/e |  1 +
  │ │   1 files changed, 1 insertions(+), 0 deletions(-)
  │ │
  │ o  3ec8b0b8bd17 E
  │ │   smallrepofolder1/foo/e |  1 +
  │ │   1 files changed, 1 insertions(+), 0 deletions(-)
  │ │
  │ o  df0e7f5dd366 D
  │ │
  o │  cbb9c8a988b5 C
  │ │   smallrepofolder1/foo/b.txt |  1 +
  │ │   1 files changed, 1 insertions(+), 0 deletions(-)
  │ │
  o │  5e3f6798b6a3 B
  ├─╯   smallrepofolder1/bar/c.txt |  1 +
  │     smallrepofolder1/foo/d     |  1 +
  │     2 files changed, 2 insertions(+), 0 deletions(-)
  │
  o  e462fc947f26 A
      smallrepofolder1/bar/b.txt |  1 +
      smallrepofolder1/foo/a.txt |  1 +
      2 files changed, 2 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(3eb23b278c44bf5d812c96f2a3211408d2a779b566984670127eebcd01fe459d)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types
