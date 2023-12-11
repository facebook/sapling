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
  > # bookmark: M master
  > EOF
  A=7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36
  B=2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3
  C=738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7
  D=7116ef2595ff4ce61ab27e3148a35960d96a969a833ec8e7225a083d2f3b3187
  E=7db0395f45e7537640e6d8d3f3b27c55664fc3d5579fa32e08bbf7253e10f135
  M=bff949d8598cbe100ecdb824a487d00b43687d48d997a9b72cfe078221d53c5c


  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID"  "$LARGE_REPO_ID" initial-import -i "$M" --version-name "$LATEST_CONFIG_VERSION_NAME"
  Checking if bff949d8598cbe100ecdb824a487d00b43687d48d997a9b72cfe078221d53c5c is already synced 1->0
  syncing bff949d8598cbe100ecdb824a487d00b43687d48d997a9b72cfe078221d53c5c
  Execution error: syncing merge commits is supported only in large to small direction
  Error: Execution failed
