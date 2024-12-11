# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ cat >> "$ACL_FILE" << ACLS
  > {
  >   "repos": {
  >     "orig": {
  >       "actions": {
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "bypass_readonly": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     },
  >     "dest": {
  >       "actions": {
  >         "read": ["SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "write": ["SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >          "bypass_readonly": ["SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     }
  >   }
  > }
  > ACLS

  $ REPOID=0 REPONAME=orig ACL_NAME=orig setup_common_config 
  $ REPOID=1 REPONAME=dest ACL_NAME=dest setup_common_config 

  $ start_and_wait_for_mononoke_server

  $ hg clone -q mono:orig orig
  $ cd orig
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
  $ with_stripped_logs mononoke_modern_sync sync-once orig dest --start-id 0 
  Running sync-once loop
  Connectign to https://localhost:$LOCAL_PORT/edenapi/
  Health check outcome: Ok(ResponseMeta { version: HTTP/2.0, status: 200, server: Some("edenapi_server"), request_id: Some("*"), tw_task_handle: None, tw_task_version: None, tw_canary_id: None, server_load: Some(1), content_length: Some(10), content_encoding: None, mononoke_host: Some("*") }) (glob)
  Found commit ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856)), parents: [], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("A"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Blob ContentId(Blake2(eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9))
  Uploading content with id: ContentId(Blake2(eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9))
  Uploading bytes: b"A"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 1 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3)), file_type: Regular, size: 4, git_lfs: FullContent }, copy_from: None })
  Blob ContentId(Blake2(be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3))
  Uploading content with id: ContentId(Blake2(be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3))
  Uploading bytes: b"abc\n"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 4 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  Manifest HgManifestId(HgNodeHash(Sha1(c1afe800646ee45232ab5e70c57247b78dbf3899)))
  Manifest HgManifestId(HgNodeHash(Sha1(53b19c5f23977836390e5880ec30fd252a311384)))
  File HgFileNodeId(HgNodeHash(Sha1(005d992c5dcf32993668f7cede29d296c494a5d9)))
  File HgFileNodeId(HgNodeHash(Sha1(f9304d84edb8a8ee2d3ce3f9de3ea944c82eba8f)))
  Found commit ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8)), parents: [ChangesetId(Blake2(ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3))], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("E"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(1b1e26f01a806e123b37492672d2756e1c25bb31f1e15cfda410c149c317e130)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Blob ContentId(Blake2(1b1e26f01a806e123b37492672d2756e1c25bb31f1e15cfda410c149c317e130))
  Uploading content with id: ContentId(Blake2(1b1e26f01a806e123b37492672d2756e1c25bb31f1e15cfda410c149c317e130))
  Uploading bytes: b"E"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("1b1e26f01a806e123b37492672d2756e1c25bb31f1e15cfda410c149c317e130"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 1 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(96475ef07b63bf02679e9964ff65f0f96883f53d0718671bd44cce830bbf2ebd)), file_type: Regular, size: 8, git_lfs: FullContent }, copy_from: None })
  Blob ContentId(Blake2(96475ef07b63bf02679e9964ff65f0f96883f53d0718671bd44cce830bbf2ebd))
  Uploading content with id: ContentId(Blake2(96475ef07b63bf02679e9964ff65f0f96883f53d0718671bd44cce830bbf2ebd))
  Uploading bytes: b"abcdefg\n"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("96475ef07b63bf02679e9964ff65f0f96883f53d0718671bd44cce830bbf2ebd"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 8 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  Manifest HgManifestId(HgNodeHash(Sha1(5e3e3ee682cdb8a61b7537cfc1a821b6283c8bb5)))
  Manifest HgManifestId(HgNodeHash(Sha1(33ac88b3b11b11c3fd33fe71cec4c8852ba2eeef)))
  File HgFileNodeId(HgNodeHash(Sha1(dba92ad67dc1f3732ab73a5f51b77129275a1724)))
  File HgFileNodeId(HgNodeHash(Sha1(b31c6c30a54b89020d5ac28a67917349512d75eb)))
  Found commit ChangesetId(Blake2(ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3)), parents: [ChangesetId(Blake2(41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5))], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("D"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(90c8e211c758a9bbcd33e463c174f1693692677cb76c7aaf4ce41aa0a29334c0)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Blob ContentId(Blake2(90c8e211c758a9bbcd33e463c174f1693692677cb76c7aaf4ce41aa0a29334c0))
  Uploading content with id: ContentId(Blake2(90c8e211c758a9bbcd33e463c174f1693692677cb76c7aaf4ce41aa0a29334c0))
  Uploading bytes: b"D"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("90c8e211c758a9bbcd33e463c174f1693692677cb76c7aaf4ce41aa0a29334c0"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 1 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(5d3bfab620332130430c7f540f9fe0b3b0079d0b9b632e0dae96a1424a7a4242)), file_type: Regular, size: 7, git_lfs: FullContent }, copy_from: None })
  Blob ContentId(Blake2(5d3bfab620332130430c7f540f9fe0b3b0079d0b9b632e0dae96a1424a7a4242))
  Uploading content with id: ContentId(Blake2(5d3bfab620332130430c7f540f9fe0b3b0079d0b9b632e0dae96a1424a7a4242))
  Uploading bytes: b"abcdef\n"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("5d3bfab620332130430c7f540f9fe0b3b0079d0b9b632e0dae96a1424a7a4242"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 7 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  Manifest HgManifestId(HgNodeHash(Sha1(553b84eb92dd53cf5d757e536be1b42e46458017)))
  Manifest HgManifestId(HgNodeHash(Sha1(fd1a9570853c1a068efbf6175c547a554015f850)))
  File HgFileNodeId(HgNodeHash(Sha1(4eec8cfdabce9565739489483b6ad93ef7657ea9)))
  File HgFileNodeId(HgNodeHash(Sha1(aae2838d921bcc14ccbb9212f4175f300fd9f2f8)))
  Found commit ChangesetId(Blake2(41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5)), parents: [ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304))], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("C"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Blob ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d))
  Uploading content with id: ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d))
  Uploading bytes: b"C"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 1 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(c86e7a7ee4c102efc1e5166dd95c1c73fcbff59dc3b04dc79fbbf3d1d10350ed)), file_type: Regular, size: 6, git_lfs: FullContent }, copy_from: Some((NonRootMPath("dir1/dir2/first"), ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304)))) })
  Blob ContentId(Blake2(c86e7a7ee4c102efc1e5166dd95c1c73fcbff59dc3b04dc79fbbf3d1d10350ed))
  Uploading content with id: ContentId(Blake2(c86e7a7ee4c102efc1e5166dd95c1c73fcbff59dc3b04dc79fbbf3d1d10350ed))
  Uploading bytes: b"abcde\n"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("c86e7a7ee4c102efc1e5166dd95c1c73fcbff59dc3b04dc79fbbf3d1d10350ed"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 6 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  Manifest HgManifestId(HgNodeHash(Sha1(144ae30be86d40d8a0617b7ec37a70e618df4840)))
  Manifest HgManifestId(HgNodeHash(Sha1(3b6d87c4e93a918020513a57279573f4325109ef)))
  File HgFileNodeId(HgNodeHash(Sha1(a2e456504a5e61f763f1a0b36a6c247c7541b2b3)))
  File HgFileNodeId(HgNodeHash(Sha1(9bad1c227e9133a5bbae1652c889406d35e6dac1)))
  Found commit ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304)), parents: [ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("B"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Blob ContentId(Blake2(55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f))
  Uploading content with id: ContentId(Blake2(55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f))
  Uploading bytes: b"B"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 1 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(fbc4b9b407225e86008840c4095edb4f66a62bad80529b6e120bfa7d605f9423)), file_type: Regular, size: 5, git_lfs: FullContent }, copy_from: None })
  Blob ContentId(Blake2(fbc4b9b407225e86008840c4095edb4f66a62bad80529b6e120bfa7d605f9423))
  Uploading content with id: ContentId(Blake2(fbc4b9b407225e86008840c4095edb4f66a62bad80529b6e120bfa7d605f9423))
  Uploading bytes: b"abcd\n"
  Upload response: [UploadToken { data: UploadTokenData { id: AnyFileContentId(ContentId(ContentId("fbc4b9b407225e86008840c4095edb4f66a62bad80529b6e120bfa7d605f9423"))), bubble_id: None, metadata: Some(FileContentTokenMetadata(FileContentTokenMetadata { content_size: 5 })) }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } }]
  Manifest HgManifestId(HgNodeHash(Sha1(83af7e770afc39d483b9cd198c49fe919ef0461a)))
  Manifest HgManifestId(HgNodeHash(Sha1(0652870aff7b4cb5e2172325519652378ae063e7)))
  File HgFileNodeId(HgNodeHash(Sha1(35e7525ce3a48913275d7061dd9a867ffef1e34d)))
  File HgFileNodeId(HgNodeHash(Sha1(778675f9ec8d35ff2fce23a34f68edd15d783853)))

  $ cat  $TESTTMP/modern_sync_scuba_logs | jq | rg "start_id|dry_run|repo"
      "start_id": 0,
      "dry_run": "false",
      "repo": "orig"* (glob)
