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
# when the large repo is cloned, there'll be a failure because hg derived data
# can't be derived for them.
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

  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID"  "$LARGE_REPO_ID" initial-import --commit "$C" --version-name "$LATEST_CONFIG_VERSION_NAME" --new-bookmark "$NEW_BOOKMARK_NAME"
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  changeset resolved as: ChangesetId(Blake2(ab5bf42dd164f61fa2bcb2de20224d8ffb60f12619bb3692f69d7c171dc1c3be))
  Checking if ab5bf42dd164f61fa2bcb2de20224d8ffb60f12619bb3692f69d7c171dc1c3be is already synced 1->0
  syncing ab5bf42dd164f61fa2bcb2de20224d8ffb60f12619bb3692f69d7c171dc1c3be
  Setting bookmark SYNCED_HEAD to changeset f299e57c379932297b130d60f6d86e54c87c8e02507bf0867783e23d7d8f8a50
  changeset ab5bf42dd164f61fa2bcb2de20224d8ffb60f12619bb3692f69d7c171dc1c3be synced as f299e57c379932297b130d60f6d86e54c87c8e02507bf0867783e23d7d8f8a50 in * (glob)
  successful sync


  $ clone_and_log_large_repo "$NEW_BOOKMARK_NAME" "$C"
  streaming all changes
  2 files to transfer, 0 bytes of data
  transferred 0 bytes in 0.0 seconds (0 bytes/sec)
  remote: Command failed
  remote:   Error:
  remote:     Git submodules not supported
  remote: 
  remote:   Root cause:
  remote:     Git submodules not supported
  remote: 
  remote:   Debug context:
  remote:     "Git submodules not supported"
  abort: unexpected EOL, expected netstring digit
  abort: '$TESTTMP/large_repo' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  abort: '$TESTTMP/large_repo' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  
  
  Running mononoke_admin to verify mapping
  
  using repo "small_repo" repoid RepositoryId(1)
  using repo "large_repo" repoid RepositoryId(0)
  changeset resolved as: ChangesetId(Blake2(ab5bf42dd164f61fa2bcb2de20224d8ffb60f12619bb3692f69d7c171dc1c3be))
  RewrittenAs([(ChangesetId(Blake2(f299e57c379932297b130d60f6d86e54c87c8e02507bf0867783e23d7d8f8a50)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
