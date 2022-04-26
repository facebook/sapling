# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_pre_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  $ blobimport repo-hg/.hg repo --derived-data-type=blame --derived-data-type=changeset_info --derived-data-type=fsnodes --derived-data-type=unodes

Output pretty debug to stdout
  $ mononoke_walker -L sizing scrub -q -b master_bookmark -I shallow -i bonsai --include-output-node-type=Changeset 2>&1 | strip_glog
  Walking edge types [BookmarkToChangeset]
  Walking node types [Bookmark, Changeset]
  Node Changeset(ChangesetKey { inner: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)), filenode_known_derived: false }): NodeData: Some(
      Changeset(
          BonsaiChangeset {
              inner: BonsaiChangesetMut {
                  parents: [
                      ChangesetId(
                          Blake2(459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66),
                      ),
                  ],
                  author: "test",
                  author_date: DateTime(
                      1970-01-01T00:00:00+00:00,
                  ),
                  committer: None,
                  committer_date: None,
                  message: "C",
                  extra: {},
                  file_changes: {
                      MPath("C"): Change(
                          TrackedFileChange {
                              inner: BasicFileChange {
                                  content_id: ContentId(
                                      Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d),
                                  ),
                                  file_type: Regular,
                                  size: 1,
                              },
                              copy_from: None,
                          },
                      ),
                  },
                  is_snapshot: false,
              },
              id: ChangesetId(
                  Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd),
              ),
          },
      ),
  )
  Seen,Loaded: 2,2
  * Type:Walked,Checks,Children Bookmark:1,1,2 Changeset:1,1,0 (glob)

Output non-pretty debug to stdout
  $ mononoke_walker -L sizing scrub -q -b master_bookmark -I shallow -i bonsai --include-output-node-type=Changeset --output-format=Debug 2>&1 | strip_glog
  Walking edge types [BookmarkToChangeset]
  Walking node types [Bookmark, Changeset]
  Node Changeset(ChangesetKey { inner: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)), filenode_known_derived: false }): NodeData: Some(Changeset(BonsaiChangeset { inner: BonsaiChangesetMut { parents: [ChangesetId(Blake2(459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66))], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: "C", extra: {}, file_changes: {MPath("C"): Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d)), file_type: Regular, size: 1 }, copy_from: None })}, is_snapshot: false }, id: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)) }))
  Seen,Loaded: 2,2
  * Type:Walked,Checks,Children Bookmark:1,1,2 Changeset:1,1,0 (glob)
