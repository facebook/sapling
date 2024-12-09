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
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"],
  >         "bypass_readonly": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"]
  >       }
  >     },
  >     "dest": {
  >       "actions": {
  >         "read": ["SERVICE_IDENTITY:server"],
  >         "write": ["SERVICE_IDENTITY:server"],
  >          "bypass_readonly": ["SERVICE_IDENTITY:server"]
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


  $ with_stripped_logs mononoke_modern_sync sync-one orig dest --cs-id 53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856
  Connectign to https://localhost:$LOCAL_PORT/edenapi/
  Health check outcome: Ok(ResponseMeta { version: HTTP/2.0, status: 200, server: Some("edenapi_server"), request_id: Some("*"), tw_task_handle: None, tw_task_version: None, tw_canary_id: None, server_load: Some(1), content_length: Some(10), content_encoding: None, mononoke_host: Some("*") }) (glob)
  Found commit ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))
  Commit info ChangesetInfo { changeset_id: ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856)), parents: [], author: "test", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: None, committer_date: None, message: Message("A"), hg_extra: {}, git_extra_headers: None }
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9)), file_type: Regular, size: 1, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9))
  File change Change(TrackedFileChange { inner: BasicFileChange { content_id: ContentId(Blake2(be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3)), file_type: Regular, size: 4, git_lfs: FullContent }, copy_from: None })
  Uploading content with id: ContentId(Blake2(be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3))

  $ mononoke_admin filestore -R orig fetch --content-id eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  A (no-eol)
  $ mononoke_admin filestore -R dest fetch --content-id eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  Error: Content not found
  [1]
