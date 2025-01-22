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
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >          "bypass_readonly": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     }
  >   },
  >   "tiers": {
  >     "mirror_commit_upload": {
  >       "actions": {
  >         "mirror_upload": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
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

  $ hg log > $TESTTMP/hglog.out

Sync all bookmarks moves
  $ with_stripped_logs mononoke_modern_sync sync-once orig dest --start-id 0 
  Running sync-once loop
  Connecting to https://localhost:$LOCAL_PORT/edenapi/
  Health check outcome: Ok(ResponseMeta { version: HTTP/2.0, status: 200, server: Some("edenapi_server"), request_id: Some("*"), tw_task_handle: None, tw_task_version: None, tw_canary_id: None, server_load: Some(1), content_length: Some(10), content_encoding: None, mononoke_host: Some("*") }) (glob)
  Processing changeset ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("e20237022b1290d98c3f14049931a8f498c18c53")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Move bookmark response SetBookmarkResponse { data: Ok(()) }
  Processing changeset ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("5a95ef0f59a992dcb5385649217862599de05565")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Processing changeset ChangesetId(Blake2(41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("fc03e5f3125836eb107f2fa5b070f841d0b62b85")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Processing changeset ChangesetId(Blake2(ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("2571175c538cc794dc974c705fcb12bc848efab4")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Processing changeset ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("8c3947e5d8bd4fe70259eca001b8885651c75850")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Move bookmark response SetBookmarkResponse { data: Ok(()) }

  $ mononoke_admin mutable-counters --repo-name orig get modern_sync
  Some(2)
  $ cat  $TESTTMP/modern_sync_scuba_logs | jq | rg "start_id|dry_run|repo"
      "start_id": 0,
      "dry_run": "false",
      "repo": "orig"* (glob)

  $ cd ..

  $ hg clone -q mono:dest dest --noupdate
  $ cd dest
  $ hg pull 
  pulling from mono:dest

  $ hg log > $TESTTMP/hglog2.out
  $ hg up master_bookmark 
  10 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls dir1/dir2
  fifth
  first
  forth
  second
  third

  $ diff  $TESTTMP/hglog.out  $TESTTMP/hglog2.out 

  $ mononoke_admin repo-info  --repo-name dest --show-commit-count
  Repo: dest
  Repo-Id: 1
  Main-Bookmark: master (not set)
  Commits: 5 (Public: 0, Draft: 5)

// Try to re-sync and hit error cause bookmark can't be re-written
  $ with_stripped_logs mononoke_modern_sync sync-once orig dest --start-id 0
  Running sync-once loop
  Connecting to https://localhost:$LOCAL_PORT/edenapi/
  Health check outcome: Ok(ResponseMeta { version: HTTP/2.0, status: 200, server: Some("edenapi_server"), request_id: Some("*"), tw_task_handle: None, tw_task_version: None, tw_canary_id: None, server_load: Some(1), content_length: Some(10), content_encoding: None, mononoke_host: Some("*") }) (glob)
  Processing changeset ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("e20237022b1290d98c3f14049931a8f498c18c53")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Move bookmark response SetBookmarkResponse { data: Ok(()) }
  Processing changeset ChangesetId(Blake2(8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("5a95ef0f59a992dcb5385649217862599de05565")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Processing changeset ChangesetId(Blake2(41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("fc03e5f3125836eb107f2fa5b070f841d0b62b85")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Processing changeset ChangesetId(Blake2(ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("2571175c538cc794dc974c705fcb12bc848efab4")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Processing changeset ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("8c3947e5d8bd4fe70259eca001b8885651c75850")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]
  Move bookmark response SetBookmarkResponse { data: Ok(()) }
