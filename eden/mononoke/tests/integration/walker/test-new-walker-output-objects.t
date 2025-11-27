# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"

  $ testtool_drawdag -R repo --derive-all << EOF
  > C
  > |
  > B
  > |
  > A
  > # bookmark: C master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Output pretty debug to stdout
  $ mononoke_walker scrub -q -b master_bookmark -I shallow -i bonsai --include-output-node-type=Changeset 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BookmarkToChangeset]
  [INFO] Walking node types [Bookmark, Changeset]
  Node Changeset(ChangesetKey { inner: ChangesetId(Blake2(e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2)), filenode_known_derived: false }): NodeData: Some(
      Changeset(
          BonsaiChangeset {
              inner: BonsaiChangesetMut {
                  parents: [
                      ChangesetId(
                          Blake2(f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658),
                      ),
                  ],
                  author: "author",
                  author_date: DateTime(
                      1970-01-01T00:00:00+00:00,
                  ),
                  committer: None,
                  committer_date: None,
                  message: "C",
                  hg_extra: {},
                  git_extra_headers: None,
                  file_changes: {
                      NonRootMPath("C"): Change(
                          TrackedFileChange {
                              inner: BasicFileChange {
                                  content_id: ContentId(
                                      Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d),
                                  ),
                                  file_type: Regular,
                                  size: 1,
                                  git_lfs: FullContent,
                              },
                              copy_from: None,
                          },
                      ),
                  },
                  is_snapshot: false,
                  git_tree_hash: None,
                  git_annotated_tag: None,
                  subtree_changes: {},
              },
              id: ChangesetId(
                  Blake2(e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2),
              ),
          },
      ),
  )
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 2,2

Output non-pretty debug to stdout
  $ mononoke_walker scrub -q -b master_bookmark -I shallow -i bonsai --include-output-node-type=Changeset --output-format=Debug 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BookmarkToChangeset]
  [INFO] Walking node types [Bookmark, Changeset]
  Node Changeset(ChangesetKey { inner: ChangesetId(Blake2(e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2)), filenode_known_derived: false }): NodeData: Some(Changeset(BonsaiChangeset { inner: BonsaiChangesetMut { parents: [ChangesetId(Blake2(f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658))], author: "author", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: "C", hg_extra: {}, git_extra_headers: None, file_changes: {NonRootMPath("C"): Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })}, is_snapshot: false, git_tree_hash: None, git_annotated_tag: None, subtree_changes: {} }, id: ChangesetId(Blake2(e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2)) }))
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 2,2
