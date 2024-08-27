# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

-- Define the large and small repo ids and names before calling any helpers
  $ export LARGE_REPO_NAME="large_repo"
  $ export LARGE_REPO_ID=10
  $ export SUBMODULE_REPO_NAME="small_repo"
  $ export SUBMODULE_REPO_ID=11

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"

Setup configuration
  $ run_common_xrepo_sync_with_gitsubmodules_setup
  L_A=b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4

# This tests the scenario where a commit contains ONLY changes to git submodules
# i.e. there are not file changes that should be synced to the large repo.
# TODO(T169315758): Handle commits changes only to git submodules
Create commit that modifies git submodule in small repo
  $ testtool_drawdag -R "$SUBMODULE_REPO_NAME" --no-default-files <<EOF
  > A-B-C
  > # modify: A "foo/a.txt" "creating foo directory"
  > # modify: A "bar/b.txt" "creating bar directory"
  > # modify: B "foo/git_submodule" git-submodule "creating git submodule"
  > # copy: C "foo/b.txt" "copying file from bar into foo" B "bar/b.txt"
  > # bookmark: C master
  > EOF
  A=7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36
  B=b51882d566acc1f3979a389e452e2c11ccdd05be65bf777c05924fc412b2cc71
  C=6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4

  $ with_stripped_logs mononoke_x_repo_sync "$SUBMODULE_REPO_ID" "$LARGE_REPO_ID" \
  > initial-import --no-progress-bar -i "$C" --version-name "$LATEST_CONFIG_VERSION_NAME" \
  > | tee $TESTTMP/initial_import.out
  Starting session with id * (glob)
  Starting up X Repo Sync from small repo small_repo to large repo large_repo
  Checking if 6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4 is already synced 11->10
  Syncing 6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4 for inital import
  Source repo: small_repo / Target repo: large_repo
  Found 3 unsynced ancestors
  changeset 6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4 synced as 5cac851d3a164f682613d6901e17a03e18afe8576145d4f5ff9dd0a51a82437f in * (glob)
  successful sync of head 6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4
  X Repo Sync execution finished from small repo small_repo to large repo large_repo

  $ SYNCED_HEAD=$(rg ".+synced as (\w+) .+" -or '$1' "$TESTTMP/initial_import.out")
  $ echo "$SYNCED_HEAD"
  5cac851d3a164f682613d6901e17a03e18afe8576145d4f5ff9dd0a51a82437f
  $ sleep 2
  $ clone_and_log_large_repo "$SYNCED_HEAD"
  o  f9abb21ba833 C
  │   smallrepofolder1/foo/b.txt |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  039696fd865f B
  │
  o  e462fc947f26 A
      smallrepofolder1/bar/b.txt |  1 +
      smallrepofolder1/foo/a.txt |  1 +
      2 files changed, 2 insertions(+), 0 deletions(-)
  
  @  54a6db91baf1 L_A
      file_in_large_repo.txt |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types
