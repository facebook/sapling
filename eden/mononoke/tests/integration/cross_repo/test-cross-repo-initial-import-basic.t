# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/cross_repo/library-git-submodules-config-setup.sh"
  $ . "${TEST_FIXTURES}/cross_repo/library-git-submodules-helpers.sh"



Setup configuration
  $ quiet run_common_xrepo_sync_with_gitsubmodules_setup

# Simple integration test for the initial-import command in the forward syncer
Create small repo commits
  $ testtool_drawdag -R "$SUBMODULE_REPO_NAME" --no-default-files <<EOF
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


  $ mononoke_x_repo_sync "$SUBMODULE_REPO_ID"  "$LARGE_REPO_ID" \
  > initial-import --no-progress-bar -i "$C" --add-mapping-to-hg-extra \
  > --version-name "$LATEST_CONFIG_VERSION_NAME" |& tee $TESTTMP/initial_import.out
  [INFO] Starting session with id * (glob)
  [INFO] Starting up X Repo Sync from small repo small_repo to large repo large_repo
  [INFO] Checking if 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7 is already synced 11->10
  [INFO] Syncing 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7 for initial import
  [INFO] Source repo: small_repo / Target repo: large_repo
  [INFO] Found 3 unsynced ancestors
  [INFO] changeset 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7 synced as 5018d85a3db49803d93474fec07b26a65f527ba14a320de37e8f48fb98086e7a * (glob)
  [INFO] successful sync of head 738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7
  [INFO] X Repo Sync execution finished from small repo small_repo to large repo large_repo

  $ SYNCED_HEAD=$(rg ".+synced as (\w+) .+" -or '$1' "$TESTTMP/initial_import.out")
  $ clone_and_log_large_repo "$SYNCED_HEAD"
  o  46cfaa628eaf C
  │   smallrepofolder1/foo/b.txt |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  03e3cfe82f43 B
  │   smallrepofolder1/bar/c.txt |  1 +
  │   smallrepofolder1/foo/d     |  1 +
  │   2 files changed, 2 insertions(+), 0 deletions(-)
  │
  o  31d9be73e63c A
      smallrepofolder1/bar/b.txt |  1 +
      smallrepofolder1/foo/a.txt |  1 +
      2 files changed, 2 insertions(+), 0 deletions(-)
  
  @  54a6db91baf1 L_A
      file_in_large_repo.txt |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(738630e43445144e9f5ddbe1869730cfbaf8ff6bf95b25b8410cb35ca92f25c7)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types






# Check that commit sync mapping is in hg extra
# $ MASTER_HASH=$(mononoke_admin bookmarks -R $LARGE_REPO_NAME get master_bookmark)
  $ mononoke_admin blobstore -R $LARGE_REPO_NAME fetch changeset.blake2.$SYNCED_HEAD
  Key: changeset.blake2.5018d85a3db49803d93474fec07b26a65f527ba14a320de37e8f48fb98086e7a
  Ctime: * (glob)
  Size: 247
  
  BonsaiChangeset {
      inner: BonsaiChangesetMut {
          parents: [
              ChangesetId(
                  Blake2(2eb103a039e10690ad71c9e784a0dd41b31e8a6ca3b6c840792bd7723237a748),
              ),
          ],
          author: "author",
          author_date: DateTime(
              1970-01-01T00:00:00+00:00,
          ),
          committer: None,
          committer_date: None,
          message: "C",
          hg_extra: {
              "sync_mapping_version": b"INITIAL_IMPORT_SYNC_CONFIG",
          },
          git_extra_headers: None,
          file_changes: {
              NonRootMPath("smallrepofolder1/foo/b.txt"): Change(
                  TrackedFileChange {
                      inner: BasicFileChange {
                          content_id: ContentId(
                              Blake2(e7bb73a12f705640aacb03fe42d56ad6884a5e74df826b08fa7fa58203d9e407),
                          ),
                          file_type: Regular,
                          size: 30,
                          git_lfs: FullContent,
                      },
                      copy_from: Some(
                          (
                              NonRootMPath("smallrepofolder1/bar/b.txt"),
                              ChangesetId(
                                  Blake2(2eb103a039e10690ad71c9e784a0dd41b31e8a6ca3b6c840792bd7723237a748),
                              ),
                          ),
                      ),
                  },
              ),
          },
          is_snapshot: false,
          git_tree_hash: None,
          git_annotated_tag: None,
          subtree_changes: {},
      },
      id: ChangesetId(
          Blake2(5018d85a3db49803d93474fec07b26a65f527ba14a320de37e8f48fb98086e7a),
      ),
  }

