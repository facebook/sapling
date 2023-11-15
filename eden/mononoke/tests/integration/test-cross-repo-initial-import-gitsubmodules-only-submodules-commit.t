# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"



Setup configuration
  $ run_common_xrepo_sync_with_gitsubmodules_setup

# This tests the scenario where a commit contains ONLY changes to git submodules
# i.e. there are not file changes that should be synced to the large repo.
# TODO(T169315758): Handle commits changes only to git submodules
Create commit that modifies git submodule in small repo
  $ testtool_drawdag -R "$SMALL_REPO_NAME" --no-default-files <<EOF
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

  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID"  "$LARGE_REPO_ID" initial-import --commit "$C" --version-name "$LATEST_CONFIG_VERSION_NAME" --new-bookmark "$NEW_BOOKMARK_NAME"
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  changeset resolved as: ChangesetId(Blake2(6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4))
  Checking if 6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4 is already synced 1->0
  syncing 6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4
  Execution error: tried to insert inconsistent small bcs id Some(ChangesetId(Blake2(b51882d566acc1f3979a389e452e2c11ccdd05be65bf777c05924fc412b2cc71))) version Some(CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG")), while db has Some(ChangesetId(Blake2(7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36))) version Some(CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))
  Execution failed
  Error: Execution failed



  $ clone_and_log_large_repo "$NEW_BOOKMARK_NAME" "$C"
  abort: unknown revision 'SYNCED_HEAD'!
  
  
  Running mononoke_admin to verify mapping
  
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  changeset resolved as: ChangesetId(Blake2(6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4))
  6473a332b6f2c52543365108144f9b1cff6b4874bc3ade72a8268f50226f86f4 is not remapped
