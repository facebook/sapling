# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config

  $ start_and_wait_for_mononoke_server

  $ hg clone -q mono:repo repo
  $ cd repo
  $ drawdag << EOS
  > E # E/dir1/dir2/fifth = abcdefg\n
  > |
  > D # D/dir1/dir2/forth = abcdef\n
  > |
  > C # C/dir1/dir2/third = abcde\n (copied from dir1/dir2/first)
  > |
  > B # B/dir1/dir2/second = abcd\n
  > |
  > A # A/dir1/dir2/first = abc\n
  > EOS


  $ hg goto A -q
  $ hg push -r . --to master_bookmark -q --create

  $ hg goto E -q
  $ hg push -r . --to master_bookmark -q

Sync all bookmarks moves
  $ with_stripped_logs mononoke_modern_sync 0 
  Running sync-once loop
  Found commit ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856)), parents: [], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("A"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9))
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3)), file_type: Regular, size: 4, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3))
  Found commit ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8)), parents: [ChangesetId(Blake2(ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3))], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("E"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(1b1e26f01a806e123b37492672d2756e1c25bb31f1e15cfda410c149c317e130)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(1b1e26f01a806e123b37492672d2756e1c25bb31f1e15cfda410c149c317e130))
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(96475ef07b63bf02679e9964ff65f0f96883f53d0718671bd44cce830bbf2ebd)), file_type: Regular, size: 8, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(96475ef07b63bf02679e9964ff65f0f96883f53d0718671bd44cce830bbf2ebd))
  Found commit ChangesetId(Blake2(ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3)), parents: [ChangesetId(Blake2(41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5))], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("D"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(90c8e211c758a9bbcd33e463c174f1693692677cb76c7aaf4ce41aa0a29334c0)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(90c8e211c758a9bbcd33e463c174f1693692677cb76c7aaf4ce41aa0a29334c0))
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(5d3bfab620332130430c7f540f9fe0b3b0079d0b9b632e0dae96a1424a7a4242)), file_type: Regular, size: 7, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(5d3bfab620332130430c7f540f9fe0b3b0079d0b9b632e0dae96a1424a7a4242))
  Found commit ChangesetId(Blake2(41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5)), parents: [ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304))], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("C"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d))
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(c86e7a7ee4c102efc1e5166dd95c1c73fcbff59dc3b04dc79fbbf3d1d10350ed)), file_type: Regular, size: 6, git_lfs: FullContent }, copy_from: Some((NonRootMPath("dir1/dir2/first"), ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304)))) })
  Uploading content with id: ContentId(Blake2(c86e7a7ee4c102efc1e5166dd95c1c73fcbff59dc3b04dc79fbbf3d1d10350ed))
  Found commit ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304)), parents: [ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("B"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f))
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(fbc4b9b407225e86008840c4095edb4f66a62bad80529b6e120bfa7d605f9423)), file_type: Regular, size: 5, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(fbc4b9b407225e86008840c4095edb4f66a62bad80529b6e120bfa7d605f9423))
