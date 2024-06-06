# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# This test will set up 3 git repos: A, B and C
# A will depend on B as a submodule and B will depend on C.
#

# The test will run an initial-import and set up a live sync from A to a large 
# repo, expanding the git submodule changes.
# All files from all submodules need to be copied in A, in the appropriate
# subdirectory.
# After that, we make more changes to the submodules, update their git repos,
# import the new commits and run the forward syncer again, to test the workflow
# one more time.

-- Define the large and small repo ids and names before calling any helpers
  $ export LARGE_REPO_NAME="large_repo"
  $ export LARGE_REPO_ID=10
  $ export SUBMODULE_REPO_NAME="small_repo"
  $ export SUBMODULE_REPO_ID=11

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-git-submodule-expansion.sh"

Avoid local clone error "fatal: transport 'file' not allowed" in new Git versions (see CVE-2022-39253).
  $ export XDG_CONFIG_HOME=$TESTTMP
  $ git config --global protocol.file.allow always


Run the x-repo with submodules setup  
  $ ENABLE_API_WRITES=1 REPOID="$REPO_C_ID" REPONAME="repo_c" setup_common_config "$REPOTYPE"
  $ ENABLE_API_WRITES=1 REPOID="$REPO_B_ID" REPONAME="repo_b" setup_common_config "$REPOTYPE"
  $ run_common_xrepo_sync_with_gitsubmodules_setup
  $ set_git_submodules_action_in_config_version "$LATEST_CONFIG_VERSION_NAME" "$SUBMODULE_REPO_ID" 3 # 3=expand
  $ set_git_submodule_dependencies_in_config_version "$LATEST_CONFIG_VERSION_NAME" \
  > "$SUBMODULE_REPO_ID" "{\"git-repo-b\": $REPO_B_ID, \"git-repo-b/git-repo-c\": $REPO_C_ID, \"repo_c\": $REPO_C_ID}"


Create a commit in the large repo
  $ testtool_drawdag -R "$LARGE_REPO_NAME" --no-default-files <<EOF
  > L_A
  > # modify: L_A "file_in_large_repo.txt" "first file"
  > # bookmark: L_A master
  > EOF
  L_A=b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4

Setup git repos A, B and C
  $ setup_git_repos_a_b_c
  
  
  NOTE: Setting up git repo C to be used as submodule in git repo B
  114b61c Add hoo/qux
  7f760d8 Add choo
  
  
  NOTE: Setting up git repo B to be used as submodule in git repo A
  Cloning into '$TESTTMP/git-repo-b/git-repo-c'...
  done.
  776166f Added git repo C as submodule in B
  b7dc5d8 Add bar/zoo
  1c7ecd4 Add foo
  .
  |-- .gitmodules
  |-- bar
  |   `-- zoo
  |-- foo
  `-- git-repo-c
      |-- choo
      `-- hoo
          `-- qux
  
  3 directories, 5 files
  
  
  NOTE: Setting up git repo A
  Cloning into '$TESTTMP/git-repo-a/git-repo-b'...
  done.
  f3ce0ee Added git repo B as submodule in A
  ad7b606 Add regular_dir/aardvar
  8c33a27 Add root_file
  Cloning into '$TESTTMP/git-repo-a/repo_c'...
  done.
  .
  |-- .gitmodules
  |-- duplicates
  |   |-- x
  |   |-- y
  |   `-- z
  |-- git-repo-b
  |   |-- .gitmodules
  |   |-- bar
  |   |   `-- zoo
  |   |-- foo
  |   `-- git-repo-c
  |-- regular_dir
  |   `-- aardvar
  |-- repo_c
  |   |-- choo
  |   `-- hoo
  |       `-- qux
  `-- root_file
  
  7 directories, 11 files

Import all git repos into Mononoke
  $ gitimport_repos_a_b_c
  
  
  NOTE: Importing repos in reverse dependency order, C, B then A
  
  GIT_REPO_A_HEAD: eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c
  
  GIT_REPO_A_HEAD_PARENT: c33eeb91423c021a4d9d57f2efbb08185c77d89b9141433c666b84240395f0c5

Merge repo A into the large repo
  $ REPO_A_FOLDER="smallrepofolder1" merge_repo_a_to_large_repo
  
  
  NOTE: Importing repo A commits into large repo
  IMPORT_CONFIG_VERSION_NAME: INITIAL_IMPORT_SYNC_CONFIG
  FINAL_CONFIG_VERSION_NAME: INITIAL_IMPORT_SYNC_CONFIG
  Large repo MASTER_BOOKMARK_NAME: master
  SMALL_REPO_FOLDER: smallrepofolder1
  
  GIT_REPO_A_HEAD: eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c
  
  GIT_REPO_A_HEAD_PARENT: c33eeb91423c021a4d9d57f2efbb08185c77d89b9141433c666b84240395f0c5
  
  
  NOTE: Running initial import
  Starting session with id * (glob)
  Checking if * (glob)
  Syncing c33eeb91423c021a4d9d57f2efbb08185c77d89b9141433c666b84240395f0c5 for inital import
  Source repo: small_repo / Target repo: large_repo
  Found * unsynced ancestors (glob)
  changeset * synced as * in * (glob)
  successful sync of head * (glob)
  
  
  NOTE: Large repo bookmarks
  54a6db91baf1c10921369339b50e5a174a7ca82e master
  
  IMPORTED_HEAD: 6e3217760eada6926186d7cb48f4f24bd8a734ad615aec528065a0912dec6cba
  
  
  
  NOTE: Creating deletion commits
  using repo "large_repo" repoid RepositoryId(10)
  changeset resolved as: ChangesetId(Blake2(6e3217760eada6926186d7cb48f4f24bd8a734ad615aec528065a0912dec6cba))
  Gathering working copy files under [NonRootMPath("smallrepofolder1")]
  13 paths to be deleted
  Starting deletion
  Chunking mpaths
  Done chunking working copy contents
  Creating delete commit #0 with ChangesetArgs { author: "test_user", message: "[MEGAREPO DELETE] deletion commits for merge into large repo (0)", datetime: DateTime(1985-09-04T00:00:00+00:00), bookmark: None, mark_public: false } (deleting 2 files)
  Done creating delete commit #0
  Creating delete commit #1 with ChangesetArgs { author: "test_user", message: "[MEGAREPO DELETE] deletion commits for merge into large repo (1)", datetime: DateTime(1985-09-04T00:00:00+00:00), bookmark: None, mark_public: false } (deleting 2 files)
  Done creating delete commit #1
  Creating delete commit #2 with ChangesetArgs { author: "test_user", message: "[MEGAREPO DELETE] deletion commits for merge into large repo (2)", datetime: DateTime(1985-09-04T00:00:00+00:00), bookmark: None, mark_public: false } (deleting 2 files)
  Done creating delete commit #2
  Creating delete commit #3 with ChangesetArgs { author: "test_user", message: "[MEGAREPO DELETE] deletion commits for merge into large repo (3)", datetime: DateTime(1985-09-04T00:00:00+00:00), bookmark: None, mark_public: false } (deleting 2 files)
  Done creating delete commit #3
  Creating delete commit #4 with ChangesetArgs { author: "test_user", message: "[MEGAREPO DELETE] deletion commits for merge into large repo (4)", datetime: DateTime(1985-09-04T00:00:00+00:00), bookmark: None, mark_public: false } (deleting 2 files)
  Done creating delete commit #4
  Creating delete commit #5 with ChangesetArgs { author: "test_user", message: "[MEGAREPO DELETE] deletion commits for merge into large repo (5)", datetime: DateTime(1985-09-04T00:00:00+00:00), bookmark: None, mark_public: false } (deleting 2 files)
  Done creating delete commit #5
  Creating delete commit #6 with ChangesetArgs { author: "test_user", message: "[MEGAREPO DELETE] deletion commits for merge into large repo (6)", datetime: DateTime(1985-09-04T00:00:00+00:00), bookmark: None, mark_public: false } (deleting 1 files)
  Done creating delete commit #6
  Deletion finished
  Listing commits in an ancestor-descendant order
  158d79bf40963db4463d2beb6b001fb718fcf363ad668baf3246639e4ee7ea0e
  871ceefaa2ea1ee69cd3c4fcd7f76c2b5cffb5bfa267209a05c6f9153b7a3446
  56b639bf49f98ba2edec100f4e1336d915edaa8e6c64583f04b0a8b5411dff46
  c197e1b3d74f56ddc1138c60bde466a42b878029761e16ee600aa63ed00d1b92
  d0f6b855e26e616df8bd56db4829a69cd8a8d918a84b2e43f2d5503a672d4136
  e59127e0c8706790dd084a50a93990d7a7d506634bf45772747ef278618fc70d
  1501ae2b26ebd41f647827f99dbe66969a4ce4d2a05802b664bf24a98263018f
  
  LAST_DELETION_COMMIT: 1501ae2b26ebd41f647827f99dbe66969a4ce4d2a05802b664bf24a98263018f
  
  
  
  NOTE: Creating gradual merge commit
  using repo "large_repo" repoid RepositoryId(10)
  changeset resolved as: ChangesetId(Blake2(1501ae2b26ebd41f647827f99dbe66969a4ce4d2a05802b664bf24a98263018f))
  changeset resolved as: ChangesetId(Blake2(6e3217760eada6926186d7cb48f4f24bd8a734ad615aec528065a0912dec6cba))
  Finding all commits to merge...
  8 total commits to merge
  Finding commits that haven't been merged yet...
  changeset resolved as: ChangesetId(Blake2(b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4))
  merging 8 commits
  Preparing to merge 1501ae2b26ebd41f647827f99dbe66969a4ce4d2a05802b664bf24a98263018f
  changeset resolved as: ChangesetId(Blake2(b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4))
  Created merge changeset b951335783b358e94a4fee905ba8cecb6ec56f3482ec5a7ea29071ce5671ff7f
  Generated hg changeset aea509f84730e4e33ee5818d074ef07ab2e84767
  Now running pushrebase...
  Pushrebased to b951335783b358e94a4fee905ba8cecb6ec56f3482ec5a7ea29071ce5671ff7f
  Preparing to merge e59127e0c8706790dd084a50a93990d7a7d506634bf45772747ef278618fc70d
  changeset resolved as: ChangesetId(Blake2(b951335783b358e94a4fee905ba8cecb6ec56f3482ec5a7ea29071ce5671ff7f))
  Created merge changeset 55a1f856eff2495cc1b11577c6fe2cf503bcbe8c813fbe3f8b187a2b1057e0e1
  Generated hg changeset c40e45f2cc7cf814695c23f13cf7e07a5fda1545
  Now running pushrebase...
  Pushrebased to 55a1f856eff2495cc1b11577c6fe2cf503bcbe8c813fbe3f8b187a2b1057e0e1
  Preparing to merge d0f6b855e26e616df8bd56db4829a69cd8a8d918a84b2e43f2d5503a672d4136
  changeset resolved as: ChangesetId(Blake2(55a1f856eff2495cc1b11577c6fe2cf503bcbe8c813fbe3f8b187a2b1057e0e1))
  Created merge changeset 1f473d38f5233bdb2fd606c0b99ce723077bbc4235a9384af07a29cba260a817
  Generated hg changeset b019b0d36eb2dd282ce738ec13fdbf3be30a77d4
  Now running pushrebase...
  Pushrebased to 1f473d38f5233bdb2fd606c0b99ce723077bbc4235a9384af07a29cba260a817
  Preparing to merge c197e1b3d74f56ddc1138c60bde466a42b878029761e16ee600aa63ed00d1b92
  changeset resolved as: ChangesetId(Blake2(1f473d38f5233bdb2fd606c0b99ce723077bbc4235a9384af07a29cba260a817))
  Created merge changeset e8895bbde976c22c3c2e86b3a366e857c867782845a35affbd387ec67fdb7b08
  Generated hg changeset 19d222a5612c8212fb1d5c9ba457254e9bb8c94c
  Now running pushrebase...
  Pushrebased to e8895bbde976c22c3c2e86b3a366e857c867782845a35affbd387ec67fdb7b08
  Preparing to merge 56b639bf49f98ba2edec100f4e1336d915edaa8e6c64583f04b0a8b5411dff46
  changeset resolved as: ChangesetId(Blake2(e8895bbde976c22c3c2e86b3a366e857c867782845a35affbd387ec67fdb7b08))
  Created merge changeset 5ed0ad43effc02129e6999075a6e1b0da9c981ef4775e0e99a295d2df1003664
  Generated hg changeset 3d5d1c2ca8842a63492b00b98510a9f6c641136c
  Now running pushrebase...
  Pushrebased to 5ed0ad43effc02129e6999075a6e1b0da9c981ef4775e0e99a295d2df1003664
  Preparing to merge 871ceefaa2ea1ee69cd3c4fcd7f76c2b5cffb5bfa267209a05c6f9153b7a3446
  changeset resolved as: ChangesetId(Blake2(5ed0ad43effc02129e6999075a6e1b0da9c981ef4775e0e99a295d2df1003664))
  Created merge changeset d4a7ef49f849a0768c0c4145e1d9e50eaefb0202fdcc6c333cdfb073eb23e377
  Generated hg changeset 67ce5c45ccfe824e860656b64370092aa899329a
  Now running pushrebase...
  Pushrebased to d4a7ef49f849a0768c0c4145e1d9e50eaefb0202fdcc6c333cdfb073eb23e377
  Preparing to merge 158d79bf40963db4463d2beb6b001fb718fcf363ad668baf3246639e4ee7ea0e
  changeset resolved as: ChangesetId(Blake2(d4a7ef49f849a0768c0c4145e1d9e50eaefb0202fdcc6c333cdfb073eb23e377))
  Created merge changeset eae1a40ede9abfef4c165886c5300a1779ac8404b91e7352c12d4d17717b50e6
  Generated hg changeset eb7057489fd5d07098a7dce76303fb661f9ff21b
  Now running pushrebase...
  Pushrebased to eae1a40ede9abfef4c165886c5300a1779ac8404b91e7352c12d4d17717b50e6
  Preparing to merge 6e3217760eada6926186d7cb48f4f24bd8a734ad615aec528065a0912dec6cba
  changeset resolved as: ChangesetId(Blake2(eae1a40ede9abfef4c165886c5300a1779ac8404b91e7352c12d4d17717b50e6))
  Created merge changeset a8d6e2b05a2537c2ac36f5e5a1bc706c15e34e456f9488ccfa9e9ac09b00b283
  Generated hg changeset c0240984981f6f70094e0cd4f42d1e33c4c86a69
  Now running pushrebase...
  Pushrebased to a8d6e2b05a2537c2ac36f5e5a1bc706c15e34e456f9488ccfa9e9ac09b00b283
  
  
  NOTE: Changing commit sync mapping version
  Starting session with id * (glob)
  changeset resolved as: ChangesetId(Blake2(eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c))
  Checking if eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c is already synced 11->10
  Changing mapping version during pushrebase to INITIAL_IMPORT_SYNC_CONFIG
  UNSAFE: Bypass working copy validation is enabled!
  1 unsynced ancestors of eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c
  Building parent override map without working copy validation to sync using synced_ancestors_versions SyncedAncestorsVersions {
      versions: {
          CommitSyncConfigVersion(
              "INITIAL_IMPORT_SYNC_CONFIG",
          ),
      },
      rewritten_ancestors: {
          ChangesetId(
              Blake2(c33eeb91423c021a4d9d57f2efbb08185c77d89b9141433c666b84240395f0c5),
          ): (
              ChangesetId(
                  Blake2(6e3217760eada6926186d7cb48f4f24bd8a734ad615aec528065a0912dec6cba),
              ),
              CommitSyncConfigVersion(
                  "INITIAL_IMPORT_SYNC_CONFIG",
              ),
          ),
      },
  }
  all validations passed with parent_mapping {
      ChangesetId(
          Blake2(6e3217760eada6926186d7cb48f4f24bd8a734ad615aec528065a0912dec6cba),
      ): ChangesetId(
          Blake2(a8d6e2b05a2537c2ac36f5e5a1bc706c15e34e456f9488ccfa9e9ac09b00b283),
      ),
  }
  UNSAFE: changing mapping version during pushrebase to INITIAL_IMPORT_SYNC_CONFIG
  syncing eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c via pushrebase for master
  changeset eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c synced as 04190d634e49d29bf87edffb012f42f9f5e49b5b66e99714f17fcd4ef3f3e294 in * (glob)
  successful sync
  
  SYNCED_HEAD: 04190d634e49d29bf87edffb012f42f9f5e49b5b66e99714f17fcd4ef3f3e294
  
  @  e2b260a2b04f Added git repo C as submodule directly in A
  │   smallrepofolder1/.gitmodules              |  3 +++
  │   smallrepofolder1/.x-repo-submodule-repo_c |  1 +
  │   smallrepofolder1/repo_c/choo              |  1 +
  │   smallrepofolder1/repo_c/hoo/qux           |  1 +
  │   4 files changed, 6 insertions(+), 0 deletions(-)
  │
  o    c0240984981f [MEGAREPO GRADUAL MERGE] gradual merge (7)
  ├─╮   smallrepofolder1/.gitmodules                  |  3 +++
  │ │   smallrepofolder1/.x-repo-submodule-git-repo-b |  1 +
  │ │   2 files changed, 4 insertions(+), 0 deletions(-)
  │ │
  │ o    eb7057489fd5 [MEGAREPO GRADUAL MERGE] gradual merge (6)
  │ ├─╮   smallrepofolder1/duplicates/x |  1 +
  │ │ │   smallrepofolder1/duplicates/y |  1 +
  │ │ │   2 files changed, 2 insertions(+), 0 deletions(-)
  │ │ │
  │ │ o    67ce5c45ccfe [MEGAREPO GRADUAL MERGE] gradual merge (5)
  │ │ ├─╮   smallrepofolder1/duplicates/z           |  1 +
  │ │ │ │   smallrepofolder1/git-repo-b/.gitmodules |  3 +++
  │ │ │ │   2 files changed, 4 insertions(+), 0 deletions(-)
  │ │ │ │
  │ │ │ o    3d5d1c2ca884 [MEGAREPO GRADUAL MERGE] gradual merge (4)
  │ │ │ ├─╮   smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c |  1 +
  │ │ │ │ │   smallrepofolder1/git-repo-b/bar/zoo                      |  1 +
  │ │ │ │ │   2 files changed, 2 insertions(+), 0 deletions(-)
  │ │ │ │ │
  │ │ │ │ o    19d222a5612c [MEGAREPO GRADUAL MERGE] gradual merge (3)
  │ │ │ │ ├─╮   smallrepofolder1/git-repo-b/foo             |  1 +
  │ │ │ │ │ │   smallrepofolder1/git-repo-b/git-repo-c/choo |  1 +
  │ │ │ │ │ │   2 files changed, 2 insertions(+), 0 deletions(-)
  │ │ │ │ │ │
  │ │ │ │ │ o    b019b0d36eb2 [MEGAREPO GRADUAL MERGE] gradual merge (2)
  │ │ │ │ │ ├─╮   smallrepofolder1/git-repo-b/git-repo-c/hoo/qux |  1 +
  │ │ │ │ │ │ │   smallrepofolder1/regular_dir/aardvar           |  1 +
  │ │ │ │ │ │ │   2 files changed, 2 insertions(+), 0 deletions(-)
  │ │ │ │ │ │ │
  │ │ │ │ │ │ o    c40e45f2cc7c [MEGAREPO GRADUAL MERGE] gradual merge (1)
  │ │ │ │ │ │ ├─╮   smallrepofolder1/root_file |  1 +
  │ │ │ │ │ │ │ │   1 files changed, 1 insertions(+), 0 deletions(-)
  │ │ │ │ │ │ │ │
  │ │ │ │ │ │ │ o    aea509f84730 [MEGAREPO GRADUAL MERGE] gradual merge (0)
  │ │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ │ o  10dab983a27f [MEGAREPO DELETE] deletion commits for merge into large repo (6)
  │ │ │ │ │ │ ├───╯   smallrepofolder1/root_file |  1 -
  │ │ │ │ │ │ │ │     1 files changed, 0 insertions(+), 1 deletions(-)
  │ │ │ │ │ │ │ │
  │ │ │ │ │ │ o │  9f34257829fb [MEGAREPO DELETE] deletion commits for merge into large repo (5)
  │ │ │ │ │ ├─╯ │   smallrepofolder1/git-repo-b/git-repo-c/hoo/qux |  1 -
  │ │ │ │ │ │   │   smallrepofolder1/regular_dir/aardvar           |  1 -
  │ │ │ │ │ │   │   2 files changed, 0 insertions(+), 2 deletions(-)
  │ │ │ │ │ │   │
  │ │ │ │ │ o   │  b3109b39500f [MEGAREPO DELETE] deletion commits for merge into large repo (4)
  │ │ │ │ ├─╯   │   smallrepofolder1/git-repo-b/foo             |  1 -
  │ │ │ │ │     │   smallrepofolder1/git-repo-b/git-repo-c/choo |  1 -
  │ │ │ │ │     │   2 files changed, 0 insertions(+), 2 deletions(-)
  │ │ │ │ │     │
  │ │ │ │ o     │  43f727449960 [MEGAREPO DELETE] deletion commits for merge into large repo (3)
  │ │ │ ├─╯     │   smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c |  1 -
  │ │ │ │       │   smallrepofolder1/git-repo-b/bar/zoo                      |  1 -
  │ │ │ │       │   2 files changed, 0 insertions(+), 2 deletions(-)
  │ │ │ │       │
  │ │ │ o       │  9d59171d496f [MEGAREPO DELETE] deletion commits for merge into large repo (2)
  │ │ ├─╯       │   smallrepofolder1/duplicates/z           |  1 -
  │ │ │         │   smallrepofolder1/git-repo-b/.gitmodules |  3 ---
  │ │ │         │   2 files changed, 0 insertions(+), 4 deletions(-)
  │ │ │         │
  │ │ o         │  5d6979a70f2b [MEGAREPO DELETE] deletion commits for merge into large repo (1)
  │ ├─╯         │   smallrepofolder1/duplicates/x |  1 -
  │ │           │   smallrepofolder1/duplicates/y |  1 -
  │ │           │   2 files changed, 0 insertions(+), 2 deletions(-)
  │ │           │
  │ o           │  c1f01db6a932 [MEGAREPO DELETE] deletion commits for merge into large repo (0)
  ├─╯           │   smallrepofolder1/.gitmodules                  |  3 ---
  │             │   smallrepofolder1/.x-repo-submodule-git-repo-b |  1 -
  │             │   2 files changed, 0 insertions(+), 4 deletions(-)
  │             │
  o             │  1f9d3769f8c2 Added git repo B as submodule in A
  │             │   smallrepofolder1/.gitmodules                             |  3 +++
  │             │   smallrepofolder1/.x-repo-submodule-git-repo-b            |  1 +
  │             │   smallrepofolder1/git-repo-b/.gitmodules                  |  3 +++
  │             │   smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c |  1 +
  │             │   smallrepofolder1/git-repo-b/bar/zoo                      |  1 +
  │             │   smallrepofolder1/git-repo-b/foo                          |  1 +
  │             │   smallrepofolder1/git-repo-b/git-repo-c/choo              |  1 +
  │             │   smallrepofolder1/git-repo-b/git-repo-c/hoo/qux           |  1 +
  │             │   8 files changed, 12 insertions(+), 0 deletions(-)
  │             │
  o             │  e2c69ce8cc11 Add regular_dir/aardvar
  │             │   smallrepofolder1/regular_dir/aardvar |  1 +
  │             │   1 files changed, 1 insertions(+), 0 deletions(-)
  │             │
  o             │  df9086c77129 Add root_file
                │   smallrepofolder1/duplicates/x |  1 +
                │   smallrepofolder1/duplicates/y |  1 +
                │   smallrepofolder1/duplicates/z |  1 +
                │   smallrepofolder1/root_file    |  1 +
                │   4 files changed, 4 insertions(+), 0 deletions(-)
                │
                o  54a6db91baf1 L_A
                    file_in_large_repo.txt |  1 +
                    1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types
  Large repo tree:
  .
  |-- file_in_large_repo.txt
  `-- smallrepofolder1
      |-- .gitmodules
      |-- .x-repo-submodule-git-repo-b
      |-- .x-repo-submodule-repo_c
      |-- duplicates
      |   |-- x
      |   |-- y
      |   `-- z
      |-- git-repo-b
      |   |-- .gitmodules
      |   |-- .x-repo-submodule-git-repo-c
      |   |-- bar
      |   |   `-- zoo
      |   |-- foo
      |   `-- git-repo-c
      |       |-- choo
      |       `-- hoo
      |           `-- qux
      |-- regular_dir
      |   `-- aardvar
      |-- repo_c
      |   |-- choo
      |   `-- hoo
      |       `-- qux
      `-- root_file
  
  9 directories, 17 files
  
  
  NOTE: Deriving all data types
  
  
  NOTE: Count underived data types
  04190d634e49d29bf87edffb012f42f9f5e49b5b66e99714f17fcd4ef3f3e294: 0
  04190d634e49d29bf87edffb012f42f9f5e49b5b66e99714f17fcd4ef3f3e294: 0
  04190d634e49d29bf87edffb012f42f9f5e49b5b66e99714f17fcd4ef3f3e294: 0

Make changes to submodule and make sure they're synced properly
  $ make_changes_to_git_repos_a_b_c
  
  
  NOTE: Make changes to repo C
  810d4f5 commit #4 in repo C
  55e8308 commit #3 in repo C
  114b61c Add hoo/qux
  7f760d8 Add choo
  
  
  NOTE: Update those changes in repo B
  From $TESTTMP/git-repo-c
     114b61c..810d4f5  master     -> origin/master
  Submodule path 'git-repo-c': checked out '810d4f53650b0fd891ad367ccfd8fa6067d93937'
  0597690 Delete files in repo B
  c9e2185 Update submodule C in repo B
  776166f Added git repo C as submodule in B
  b7dc5d8 Add bar/zoo
  1c7ecd4 Add foo
  
  
  NOTE: Update those changes in repo A
  
  
  NOTE: Update submodule b in A
  From $TESTTMP/git-repo-b
     776166f..0597690  master     -> origin/master
  Submodule path 'git-repo-b': checked out '0597690a839ce11a250139dae33ee85d9772a47a'
  From $TESTTMP/git-repo-c
     114b61c..810d4f5  master     -> origin/master
  Submodule path 'repo_c': checked out '810d4f53650b0fd891ad367ccfd8fa6067d93937'
  
  
  NOTE: Then delete repo C submodule used directly in repo A
  Cleared directory 'repo_c'
  Submodule 'repo_c' (../git-repo-c) unregistered for path 'repo_c'
  rm 'repo_c'
  6775096 Remove repo C submodule from repo A
  5f6b001 Update submodule B in repo A
  de77178 Change directly in A
  3a41dad Added git repo C as submodule directly in A
  f3ce0ee Added git repo B as submodule in A
  ad7b606 Add regular_dir/aardvar
  8c33a27 Add root_file

  $ mononoke_newadmin bookmarks -R "$SUBMODULE_REPO_NAME" list -S hg
  heads/master

Import the changes from the git repos B and C into their Mononoke repos
  $ REPOID="$REPO_C_ID" QUIET_LOGGING_LOG_FILE="$TESTTMP/gitimport_repo_c.out"  \
  > quiet gitimport "$GIT_REPO_C" --bypass-derived-data-backfilling \
  > --bypass-readonly --generate-bookmarks missing-for-commit "$GIT_REPO_C_HEAD"

  $ REPOID="$REPO_B_ID" QUIET_LOGGING_LOG_FILE="$TESTTMP/gitimport_repo_b.out" \
  > quiet gitimport "$GIT_REPO_B" --bypass-derived-data-backfilling \
  > --bypass-readonly --generate-bookmarks missing-for-commit "$GIT_REPO_B_HEAD"

Set up live forward syncer, which should sync all commits in small repo's (repo A)
heads/master bookmark to large repo's master bookmark via pushrebase
  $ touch $TESTTMP/xreposync.out
  $ with_stripped_logs mononoke_x_repo_sync_forever "$SUBMODULE_REPO_ID" "$LARGE_REPO_ID" 

Import the changes from git repo A into its Mononoke repo. They should be automatically
forward synced to the large repo
  $ REPOID="$SUBMODULE_REPO_ID" with_stripped_logs gitimport "$GIT_REPO_A" --bypass-derived-data-backfilling \
  > --bypass-readonly --generate-bookmarks missing-for-commit "$GIT_REPO_A_HEAD" > $TESTTMP/gitimport_output

  $ QUIET_LOGGING_LOG_FILE="$TESTTMP/xrepo_sync_last_logs.out" with_stripped_logs wait_for_xrepo_sync 2

  $ cd "$TESTTMP/$LARGE_REPO_NAME"
  $ hg pull -q 
  $ hg co -q master

  $ hg log --graph -T '{node} {desc}\n' -r "all()"
  @  d246b01a5a5baff205958295aa764916ae288291 Remove repo C submodule from repo A
  │
  o  d3dae76d4349c88c24d60fe533bd9fbd02ddd5ae Update submodule B in repo A
  │
  o  ada44b220ff885a5757bf80bee03e64f0b0e063d Change directly in A
  │
  o  e2b260a2b04f485be16d9a59594dce5f2b652ea2 Added git repo C as submodule directly in A
  │
  o    c0240984981f6f70094e0cd4f42d1e33c4c86a69 [MEGAREPO GRADUAL MERGE] gradual merge (7)
  ├─╮
  │ o    eb7057489fd5d07098a7dce76303fb661f9ff21b [MEGAREPO GRADUAL MERGE] gradual merge (6)
  │ ├─╮
  │ │ o    67ce5c45ccfe824e860656b64370092aa899329a [MEGAREPO GRADUAL MERGE] gradual merge (5)
  │ │ ├─╮
  │ │ │ o    3d5d1c2ca8842a63492b00b98510a9f6c641136c [MEGAREPO GRADUAL MERGE] gradual merge (4)
  │ │ │ ├─╮
  │ │ │ │ o    19d222a5612c8212fb1d5c9ba457254e9bb8c94c [MEGAREPO GRADUAL MERGE] gradual merge (3)
  │ │ │ │ ├─╮
  │ │ │ │ │ o    b019b0d36eb2dd282ce738ec13fdbf3be30a77d4 [MEGAREPO GRADUAL MERGE] gradual merge (2)
  │ │ │ │ │ ├─╮
  │ │ │ │ │ │ o    c40e45f2cc7cf814695c23f13cf7e07a5fda1545 [MEGAREPO GRADUAL MERGE] gradual merge (1)
  │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ o    aea509f84730e4e33ee5818d074ef07ab2e84767 [MEGAREPO GRADUAL MERGE] gradual merge (0)
  │ │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ │ o  10dab983a27fce66a4c9852d40c4fd36618d63a7 [MEGAREPO DELETE] deletion commits for merge into large repo (6)
  │ │ │ │ │ │ ├───╯
  │ │ │ │ │ │ o │  9f34257829fbf29611c4bdc4b4e48c993c72d2e6 [MEGAREPO DELETE] deletion commits for merge into large repo (5)
  │ │ │ │ │ ├─╯ │
  │ │ │ │ │ o   │  b3109b39500ffcbb09a22bea594d32957e28b0e3 [MEGAREPO DELETE] deletion commits for merge into large repo (4)
  │ │ │ │ ├─╯   │
  │ │ │ │ o     │  43f727449960cc7effbf84da6e54a6daf4f77d99 [MEGAREPO DELETE] deletion commits for merge into large repo (3)
  │ │ │ ├─╯     │
  │ │ │ o       │  9d59171d496f660ee0276013e446d5687b69394f [MEGAREPO DELETE] deletion commits for merge into large repo (2)
  │ │ ├─╯       │
  │ │ o         │  5d6979a70f2b49a7fe30cabdbb771804bec798ae [MEGAREPO DELETE] deletion commits for merge into large repo (1)
  │ ├─╯         │
  │ o           │  c1f01db6a93222463fad3133b5eb89809d414cde [MEGAREPO DELETE] deletion commits for merge into large repo (0)
  ├─╯           │
  o             │  1f9d3769f8c22b50db3ed0105c9d0e9490bbe7e9 Added git repo B as submodule in A
  │             │
  o             │  e2c69ce8cc11691984e50e6023f4bbf4271aa4c3 Add regular_dir/aardvar
  │             │
  o             │  df9086c771290c305c738040313bf1cc5759eba9 Add root_file
                │
                o  54a6db91baf1c10921369339b50e5a174a7ca82e L_A
  

Check that deletions were made properly, i.e. submodule in repo_c was entirely
deleted and the files deleted in repo B were deleted inside its copy.
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .
  commit: d246b01a5a5baff205958295aa764916ae288291
  Remove repo C submodule from repo A
   smallrepofolder1/.gitmodules              |  3 ---
   smallrepofolder1/.x-repo-submodule-repo_c |  1 -
   smallrepofolder1/repo_c/choo              |  1 -
   smallrepofolder1/repo_c/choo3             |  1 -
   smallrepofolder1/repo_c/choo4             |  1 -
   smallrepofolder1/repo_c/hoo/qux           |  1 -
   6 files changed, 0 insertions(+), 8 deletions(-)
  


TODO(T174902563): Fix deletion of submodules in EXPAND submodule action.
  $ tree -a -I ".hg" &> ${TESTTMP}/large_repo_tree_2
  $ diff -y -t -T ${TESTTMP}/large_repo_tree_1 ${TESTTMP}/large_repo_tree_2
  .                                                                  .
  |-- file_in_large_repo.txt                                         |-- file_in_large_repo.txt
  `-- smallrepofolder1                                               `-- smallrepofolder1
      |-- .gitmodules                                                    |-- .gitmodules
      |-- .x-repo-submodule-git-repo-b                                   |-- .x-repo-submodule-git-repo-b
      |-- .x-repo-submodule-repo_c                                <
      |-- duplicates                                                     |-- duplicates
      |   |-- x                                                          |   |-- x
      |   |-- y                                                          |   |-- y
      |   `-- z                                                          |   `-- z
      |-- git-repo-b                                                     |-- git-repo-b
      |   |-- .gitmodules                                                |   |-- .gitmodules
      |   |-- .x-repo-submodule-git-repo-c                               |   |-- .x-repo-submodule-git-repo-c
      |   |-- bar                                                 <
      |   |   `-- zoo                                             <
      |   |-- foo                                                 <
      |   `-- git-repo-c                                                 |   `-- git-repo-c
      |       |-- choo                                                   |       |-- choo
                                                                  >      |       |-- choo3
                                                                  >      |       |-- choo4
      |       `-- hoo                                                    |       `-- hoo
      |           `-- qux                                                |           `-- qux
      |-- regular_dir                                                    |-- regular_dir
      |   `-- aardvar                                                    |   `-- aardvar
      |-- repo_c                                                  <
      |   |-- choo                                                <
      |   `-- hoo                                                 <
      |       `-- qux                                             <
      `-- root_file                                                      `-- root_file
  
  9 directories, 17 files                                         |  6 directories, 14 files
  [1]

Check that the diff that updates the submodule generates the correct delta
(i.e. instead of copying the entire working copy of the submodule every time)
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .^
  commit: d3dae76d4349c88c24d60fe533bd9fbd02ddd5ae
  Update submodule B in repo A
   smallrepofolder1/.x-repo-submodule-git-repo-b            |  2 +-
   smallrepofolder1/.x-repo-submodule-repo_c                |  2 +-
   smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c |  2 +-
   smallrepofolder1/git-repo-b/bar/zoo                      |  1 -
   smallrepofolder1/git-repo-b/foo                          |  1 -
   smallrepofolder1/git-repo-b/git-repo-c/choo3             |  1 +
   smallrepofolder1/git-repo-b/git-repo-c/choo4             |  1 +
   smallrepofolder1/repo_c/choo3                            |  1 +
   smallrepofolder1/repo_c/choo4                            |  1 +
   9 files changed, 7 insertions(+), 5 deletions(-)
  
  $ cat smallrepofolder1/.x-repo-submodule-git-repo-b
  0597690a839ce11a250139dae33ee85d9772a47a (no-eol)

Also check that our two binaries that can verify working copy are able to deal with expansions
  $ REPOIDLARGE=$LARGE_REPO_ID REPOIDSMALL=$SUBMODULE_REPO_ID verify_wc master |& strip_glog

The check-push-redirection-prereqs should behave the same both ways but let's verify it (we had bugs where it didn't)
(those outputs are still not correct but that's expected)
  $ quiet_grep "all is well" -- with_stripped_logs megarepo_tool_multirepo --source-repo-id $SUBMODULE_REPO_ID --target-repo-id $LARGE_REPO_ID check-push-redirection-prereqs "heads/master" "master" "$LATEST_CONFIG_VERSION_NAME" | strip_glog | tee $TESTTMP/push_redir_prereqs_small_large
  all is well!

  $ quiet_grep "all is well" -- with_stripped_logs megarepo_tool_multirepo --source-repo-id $LARGE_REPO_ID --target-repo-id $SUBMODULE_REPO_ID check-push-redirection-prereqs "master" "heads/master" "$LATEST_CONFIG_VERSION_NAME" | strip_glog | tee $TESTTMP/push_redir_prereqs_large_small
  all is well!
  $ diff -wbBdu $TESTTMP/push_redir_prereqs_small_large $TESTTMP/push_redir_prereqs_large_small

Let's corrupt the expansion and check if validation complains
(those outputs are still not correct but that's expected)
  $ echo corrupt > smallrepofolder1/git-repo-b/git-repo-c/choo3 
  $ echo corrupt > smallrepofolder1/.x-repo-submodule-git-repo-b
  $ hg commit -m "submodule corruption"
  $ hg push -q --to master
  $ quiet_grep "mismatch" -- megarepo_tool_multirepo --source-repo-id $SUBMODULE_REPO_ID --target-repo-id $LARGE_REPO_ID check-push-redirection-prereqs "heads/master" "master" "$LATEST_CONFIG_VERSION_NAME" | strip_glog | tee $TESTTMP/push_redir_prereqs_small_large
  submodule expansion mismatch: Failed to fetch content from content id 06a434694d9172d617062abd92f015f73978fb17dd6bcc54e708cd2c6f247970 file containing the submodule's git commit hash

  $ quiet_grep "mismatch" -- megarepo_tool_multirepo --source-repo-id $LARGE_REPO_ID --target-repo-id $SUBMODULE_REPO_ID check-push-redirection-prereqs "master" "heads/master" "$LATEST_CONFIG_VERSION_NAME" | sort | tee $TESTTMP/push_redir_prereqs_large_small
  submodule expansion mismatch: Failed to fetch content from content id 06a434694d9172d617062abd92f015f73978fb17dd6bcc54e708cd2c6f247970 file containing the submodule's git commit hash

  $ diff -wbBdu $TESTTMP/push_redir_prereqs_small_large $TESTTMP/push_redir_prereqs_large_small

-- ------------------------------------------------------------------------------
-- Test hgedenapi xrepo lookup with commits that are synced

-- Helper function to look for the mapping in the database using admin and then
-- call hgedenpi committranslateids endpoint from large to small.
  $ function check_mapping_and_run_xrepo_lookup_large_to_small {
  >   local hg_hash=$1; shift;
  >   
  >   printf "Check mapping in database with Mononoke admin\n"
  >   with_stripped_logs mononoke_admin_source_target $LARGE_REPO_ID $SUBMODULE_REPO_ID \
  >     crossrepo map $hg_hash | rg -v "using repo"
  >   
  >   printf "\n\nCall hgedenapi committranslateids\n" 
  >   
  >   REPONAME=$LARGE_REPO_NAME hgedenapi debugapi -e committranslateids \
  >     -i "[{'Hg': '$hg_hash'}]" -i "'Bonsai'" -i None -i "'$SUBMODULE_REPO_NAME'"
  >   
  > }


-- Looking up synced commits from large to small.
-- EXPECT: all of them should return the same value as mapping check using admin

-- Commit: Change directly in A
  $ check_mapping_and_run_xrepo_lookup_large_to_small ada44b220ff885a5757bf80bee03e64f0b0e063d
  Check mapping in database with Mononoke admin
  changeset resolved as: ChangesetId(Blake2(b382cdfd9ad4ee0cc977e2263ff392900cd41b1141ddf046cf93d5ef1136f0e7))
  RewrittenAs([(ChangesetId(Blake2(4aee0499ea629ebcd9d0e4be89267d7a4eab5e72f988c20a392d59081db0c32a)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  
  Call hgedenapi committranslateids
  [{"commit": {"Hg": bin("ada44b220ff885a5757bf80bee03e64f0b0e063d")},
    "translated": {"Bonsai": bin("4aee0499ea629ebcd9d0e4be89267d7a4eab5e72f988c20a392d59081db0c32a")}}]

-- Commit: Update submodule B in repo A
  $ check_mapping_and_run_xrepo_lookup_large_to_small d3dae76d4349c88c24d60fe533bd9fbd02ddd5ae
  Check mapping in database with Mononoke admin
  changeset resolved as: ChangesetId(Blake2(0617acae68a70aff4e62d0afc707785bd7b0318f912d9a83c35f99d6e0c79158))
  RewrittenAs([(ChangesetId(Blake2(b86f7426fc1fe95e22b6bef591e7ba9c8385b86f7b85abd3a377f941d39522af)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  
  Call hgedenapi committranslateids
  [{"commit": {"Hg": bin("d3dae76d4349c88c24d60fe533bd9fbd02ddd5ae")},
    "translated": {"Bonsai": bin("b86f7426fc1fe95e22b6bef591e7ba9c8385b86f7b85abd3a377f941d39522af")}}]

-- Check an original commit from small repo (before merge)
-- Commit: Add regular_dir/aardvar
  $ check_mapping_and_run_xrepo_lookup_large_to_small e2c69ce8cc11691984e50e6023f4bbf4271aa4c3
  Check mapping in database with Mononoke admin
  changeset resolved as: ChangesetId(Blake2(b43576e9e9685513cf91adbe5fb817cbe2837ba9a4dca12a4c64a6aebfe09780))
  RewrittenAs([(ChangesetId(Blake2(856b09638e2550d912282c5a9e8bd47fdf1a899545f9f4a05430a8dc7be1f768)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  
  Call hgedenapi committranslateids
  [{"commit": {"Hg": bin("e2c69ce8cc11691984e50e6023f4bbf4271aa4c3")},
    "translated": {"Bonsai": bin("856b09638e2550d912282c5a9e8bd47fdf1a899545f9f4a05430a8dc7be1f768")}}]


-- ------------------------------------------------------------------------------
-- Test backsyncing (i.e. large to small)

  $ cd "$TESTTMP/$LARGE_REPO_NAME" || exit
  $ hg pull -q && hg co -q master  
  $ hgmn status
  $ hgmn co -q .^ # go before the commit that corrupts submodules
  $ hgmn status
  $ enable commitcloud infinitepush # to push commits to server
  $ function hg_log() { 
  >   hgmn log --graph -T '{node|short} {desc}\n' "$@" 
  > }

  $ hg_log
  o  cd7933d8ab7a submodule corruption
  │
  @  d246b01a5a5b Remove repo C submodule from repo A
  │
  o  d3dae76d4349 Update submodule B in repo A
  │
  o  ada44b220ff8 Change directly in A
  │
  o  e2b260a2b04f Added git repo C as submodule directly in A
  │
  o    c0240984981f [MEGAREPO GRADUAL MERGE] gradual merge (7)
  ├─╮
  │ o    eb7057489fd5 [MEGAREPO GRADUAL MERGE] gradual merge (6)
  │ ├─╮
  │ │ o    67ce5c45ccfe [MEGAREPO GRADUAL MERGE] gradual merge (5)
  │ │ ├─╮
  │ │ │ o    3d5d1c2ca884 [MEGAREPO GRADUAL MERGE] gradual merge (4)
  │ │ │ ├─╮
  │ │ │ │ o    19d222a5612c [MEGAREPO GRADUAL MERGE] gradual merge (3)
  │ │ │ │ ├─╮
  │ │ │ │ │ o    b019b0d36eb2 [MEGAREPO GRADUAL MERGE] gradual merge (2)
  │ │ │ │ │ ├─╮
  │ │ │ │ │ │ o    c40e45f2cc7c [MEGAREPO GRADUAL MERGE] gradual merge (1)
  │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ o    aea509f84730 [MEGAREPO GRADUAL MERGE] gradual merge (0)
  │ │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ │ o  10dab983a27f [MEGAREPO DELETE] deletion commits for merge into large repo (6)
  │ │ │ │ │ │ ├───╯
  │ │ │ │ │ │ o │  9f34257829fb [MEGAREPO DELETE] deletion commits for merge into large repo (5)
  │ │ │ │ │ ├─╯ │
  │ │ │ │ │ o   │  b3109b39500f [MEGAREPO DELETE] deletion commits for merge into large repo (4)
  │ │ │ │ ├─╯   │
  │ │ │ │ o     │  43f727449960 [MEGAREPO DELETE] deletion commits for merge into large repo (3)
  │ │ │ ├─╯     │
  │ │ │ o       │  9d59171d496f [MEGAREPO DELETE] deletion commits for merge into large repo (2)
  │ │ ├─╯       │
  │ │ o         │  5d6979a70f2b [MEGAREPO DELETE] deletion commits for merge into large repo (1)
  │ ├─╯         │
  │ o           │  c1f01db6a932 [MEGAREPO DELETE] deletion commits for merge into large repo (0)
  ├─╯           │
  o             │  1f9d3769f8c2 Added git repo B as submodule in A
  │             │
  o             │  e2c69ce8cc11 Add regular_dir/aardvar
  │             │
  o             │  df9086c77129 Add root_file
                │
                o  54a6db91baf1 L_A
  

  $ tree
  .
  |-- file_in_large_repo.txt
  `-- smallrepofolder1
      |-- duplicates
      |   |-- x
      |   |-- y
      |   `-- z
      |-- git-repo-b
      |   `-- git-repo-c
      |       |-- choo
      |       |-- choo3
      |       |-- choo4
      |       `-- hoo
      |           `-- qux
      |-- regular_dir
      |   `-- aardvar
      `-- root_file
  
  6 directories, 10 files
  $ function backsync_get_info_and_derive_data() {
  >   REPONAME="$LARGE_REPO_NAME" hgedenapi cloud backup -q
  >   COMMIT_TO_SYNC=$(hgmn whereami)
  >   COMMIT_TITLE=$(hgmn log -l1  -T "{truncate(desc, 1)}")
  >   printf "Processing commit: $COMMIT_TITLE\n"
  >   printf "Commit hash: $COMMIT_TO_SYNC\n"
  >   
  >   (check_mapping_and_run_xrepo_lookup_large_to_small \
  >     $COMMIT_TO_SYNC && echo "Success!") 2>&1 | tee $TESTTMP/lookup_commit \
  >     | rg "error|Success" || true;
  >   
  >   # Return early if sync fails
  >   SYNC_EXIT_CODE=${PIPESTATUS[0]}
  >   if [ $SYNC_EXIT_CODE -ne 0 ]; then return $SYNC_EXIT_CODE; fi
  >   SYNCED_BONSAI=$(rg '"translated": \{"Bonsai": bin\("(\w+)"\)\}\}\]' -or '$1' $TESTTMP/lookup_commit);
  >   
  >   printf "\n\nSubmodule repo commit info using newadmin:\n"
  >   mononoke_newadmin fetch -R "$SUBMODULE_REPO_NAME" -i "$SYNCED_BONSAI" \
  >     | rg -v "Author"
  > 
  >   printf "\n\nDeriving all enabled types except hgchangesets and filenodes\n";
  >   (mononoke_newadmin derived-data -R "$SUBMODULE_REPO_NAME" derive -i $SYNCED_BONSAI \
  >     -T fsnodes -T unodes -T fastlog -T fsnodes -T blame -T changeset_info \
  >     -T skeleton_manifests -T deleted_manifest -T bssm_v3 \
  >     -T git_commits -T git_trees -T git_delta_manifests \
  >       && echo "Success!") 2>&1 | rg "Error|Success" || true;
  > }

-- Change a large repo file and try to backsync it to small repo
-- EXPECT: commit isn't synced and returns working copy equivalent instead
  $ echo "changing large repo file" > file_in_large_repo.txt
  $ hgmn commit -A -m "Changing large repo file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Changing large repo file
  Commit hash: 48021e7aeafd324f9976f551aea60aa88dd9f61a
  Success!
  
  
  Submodule repo commit info using newadmin:
  BonsaiChangesetId: de0a58fea04aaf7e162bcb87017752be9d3c838525df6d75a0b897ffaa068a28
  Message: Remove repo C submodule from repo A
  
  FileChanges:
  	 ADDED/MODIFIED: .gitmodules f98d40341818ca2b4b820319487d7f21ebf2f4ea2b4e2d45bab2100f212f2d49
  	 REMOVED: repo_c
  
  
  
  Deriving all enabled types except hgchangesets and filenodes
  Success!

-- Change a small repo file outside of a submodule expansion
-- EXPECT: commit is backsynced normally because it doesn't touch submodule expansions
  $ echo "changing small repo file" > smallrepofolder1/regular_dir/aardvar
  $ hgmn commit -A -m "Changing small repo in large repo (not submodule)" 
  $ backsync_get_info_and_derive_data
  Processing commit: Changing small repo in large repo (not submodule)
  Commit hash: 35e70dc7f37c3f51876a0f017a733a13809bef32
  Success!
  
  
  Submodule repo commit info using newadmin:
  BonsaiChangesetId: ee442222a80354fc6e4b8dc910d9938b73a9780608f1762ccd9836dbf2319422
  Message: Changing small repo in large repo (not submodule)
  FileChanges:
  	 ADDED/MODIFIED: regular_dir/aardvar 58186314bed8b207f5f63a4a58aa858e715f25225a6fcb68e93c12f731b801b1
  
  
  
  Deriving all enabled types except hgchangesets and filenodes
  Success!

-- -----------------------------------------------------------------------------
-- Test backsyncing changes that affect submodule expansions, which is 
-- not supported yet.
-- ALL SCENARIOS BELOW SHOULD FAIL TO BACKSYNC
-- -----------------------------------------------------------------------------

TODO(T179530927): properly support backsyncing with submodule expansion

-- Change a small repo file inside a submodule expansion
  $ echo "changing submodule expansion" > smallrepofolder1/git-repo-b/foo
  $ hgmn commit -A -m "Changing submodule expansion in large repo" 
  adding smallrepofolder1/git-repo-b/foo
  $ backsync_get_info_and_derive_data
  Processing commit: Changing submodule expansion in large repo
  Commit hash: db55fcf4988d8cc9dd6416ba487ae81d33a42bd5
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]
 

-- Change a small repo file inside a recursive submodule expansion
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "changing submodule expansion" > smallrepofolder1/git-repo-b/git-repo-c/choo
  $ hgmn commit -A -m "Changing recursive submodule expansion in large repo" 
  $ backsync_get_info_and_derive_data
  Processing commit: Changing recursive submodule expansion in large repo
  Commit hash: dc66187b1f1b6f752b611d8c6401bdf4141263f3
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]

-- Delete submodule metadata file
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ rm smallrepofolder1/.x-repo-submodule-git-repo-b
  $ hgmn commit -q -A -m "Deleting repo_b submodule metadata file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Deleting repo_b submodule metadata file
  Commit hash: fe1dbb2ac6e376f872c9b8add908feb87cc29b22
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]


-- Delete recursive submodule metadata file
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ rm smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c
  $ hgmn commit -q -A -m "Deleting repo_c recursive submodule metadata file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Deleting repo_c recursive submodule metadata file
  Commit hash: d617f2af29e47136e0a6ef94cbe950f5a595e6b6
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]


-- Modify submodule metadata file
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "change metadata file" > smallrepofolder1/.x-repo-submodule-git-repo-b
  $ hgmn commit -q -A -m "Change repo_b submodule metadata file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Change repo_b submodule metadata file
  Commit hash: 5985da70f061dd858117a3245fcbe204978e74e4
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]


-- Modify recursive submodule metadata file
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "change metadata file" > smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c
  $ hgmn commit -q -A -m "Change repo_c recursive submodule metadata file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Change repo_c recursive submodule metadata file
  Commit hash: 790afb00eb683346ab34c4d059d9c6bcfe204992
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]



-- Delete submodule expansion
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ rm -rf smallrepofolder1/git-repo-b
  $ hgmn commit -q -A -m "Delete repo_b submodule expansion" 
  $ backsync_get_info_and_derive_data
  Processing commit: Delete repo_b submodule expansion
  Commit hash: 3b874eef36932dd81043218728942692ac15ed82
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]

-- Delete recursive submodule expansion
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ rm -rf smallrepofolder1/git-repo-b/git-repo-c
  $ hgmn commit -q -A -m "Delete repo_c recursive submodule expansion" 
  $ backsync_get_info_and_derive_data
  Processing commit: Delete repo_c recursive submodule expansion
  Commit hash: 2cca54b932cd84ca0469ed3f7e971455ad0e7bd7
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]



  $ hg_log -r "sort(all(), desc)"
  @  2cca54b932cd Delete repo_c recursive submodule expansion
  │
  │ o  3b874eef3693 Delete repo_b submodule expansion
  ├─╯
  │ o  790afb00eb68 Change repo_c recursive submodule metadata file
  ├─╯
  │ o  5985da70f061 Change repo_b submodule metadata file
  ├─╯
  │ o  d617f2af29e4 Deleting repo_c recursive submodule metadata file
  ├─╯
  │ o  fe1dbb2ac6e3 Deleting repo_b submodule metadata file
  ├─╯
  │ o  dc66187b1f1b Changing recursive submodule expansion in large repo
  ├─╯
  │ o  db55fcf4988d Changing submodule expansion in large repo
  ├─╯
  o  35e70dc7f37c Changing small repo in large repo (not submodule)
  │
  o  48021e7aeafd Changing large repo file
  │
  │ o  cd7933d8ab7a submodule corruption
  ├─╯
  o  d246b01a5a5b Remove repo C submodule from repo A
  │
  o  d3dae76d4349 Update submodule B in repo A
  │
  o  ada44b220ff8 Change directly in A
  │
  o  e2b260a2b04f Added git repo C as submodule directly in A
  │
  o    c0240984981f [MEGAREPO GRADUAL MERGE] gradual merge (7)
  ├─╮
  │ o    eb7057489fd5 [MEGAREPO GRADUAL MERGE] gradual merge (6)
  │ ├─╮
  │ │ o    67ce5c45ccfe [MEGAREPO GRADUAL MERGE] gradual merge (5)
  │ │ ├─╮
  │ │ │ o    3d5d1c2ca884 [MEGAREPO GRADUAL MERGE] gradual merge (4)
  │ │ │ ├─╮
  │ │ │ │ o    19d222a5612c [MEGAREPO GRADUAL MERGE] gradual merge (3)
  │ │ │ │ ├─╮
  │ │ │ │ │ o    b019b0d36eb2 [MEGAREPO GRADUAL MERGE] gradual merge (2)
  │ │ │ │ │ ├─╮
  │ │ │ │ │ │ o    c40e45f2cc7c [MEGAREPO GRADUAL MERGE] gradual merge (1)
  │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ o    aea509f84730 [MEGAREPO GRADUAL MERGE] gradual merge (0)
  │ │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ │ o  10dab983a27f [MEGAREPO DELETE] deletion commits for merge into large repo (6)
  │ │ │ │ │ │ ├───╯
  │ │ │ │ │ │ o │  9f34257829fb [MEGAREPO DELETE] deletion commits for merge into large repo (5)
  │ │ │ │ │ ├─╯ │
  │ │ │ │ │ o   │  b3109b39500f [MEGAREPO DELETE] deletion commits for merge into large repo (4)
  │ │ │ │ ├─╯   │
  │ │ │ │ o     │  43f727449960 [MEGAREPO DELETE] deletion commits for merge into large repo (3)
  │ │ │ ├─╯     │
  │ │ │ o       │  9d59171d496f [MEGAREPO DELETE] deletion commits for merge into large repo (2)
  │ │ ├─╯       │
  │ │ o         │  5d6979a70f2b [MEGAREPO DELETE] deletion commits for merge into large repo (1)
  │ ├─╯         │
  │ o           │  c1f01db6a932 [MEGAREPO DELETE] deletion commits for merge into large repo (0)
  ├─╯           │
  o             │  1f9d3769f8c2 Added git repo B as submodule in A
  │             │
  o             │  e2c69ce8cc11 Add regular_dir/aardvar
  │             │
  o             │  df9086c77129 Add root_file
                │
                o  54a6db91baf1 L_A
  
