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


  $ with_stripped_logs mononoke_modern_sync sync-one orig dest --cs-id 53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856
  Connecting to https://localhost:$LOCAL_PORT/edenapi/
  Health check outcome: Ok(ResponseMeta { version: HTTP/2.0, status: 200, server: Some("edenapi_server"), request_id: Some("*"), tw_task_handle: None, tw_task_version: None, tw_canary_id: None, server_load: Some(1), content_length: Some(10), content_encoding: None, mononoke_host: Some("*") }) (glob)
  Processing changeset ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))
  Uploaded 2 contents successfully
  Uploaded 3 trees successfully
  Uploaded 2 filenodes successfully
  Upload hg changeset response: [UploadTokensResponse { token: UploadToken { data: UploadTokenData { id: HgChangesetId(HgId("e20237022b1290d98c3f14049931a8f498c18c53")), bubble_id: None, metadata: None }, signature: UploadTokenSignature { signature: [102, 97, 107, 101, 116, 111, 107, 101, 110, 115, 105, 103, 110, 97, 116, 117, 114, 101] } } }]

Compare content
  $ diff <(mononoke_admin filestore -R orig fetch --content-id eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9) <(mononoke_admin filestore -R dest fetch --content-id eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9)
  $ diff <(mononoke_admin filestore -R orig fetch --content-id be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3) <(mononoke_admin filestore -R dest fetch --content-id be87911855af0fc33a75f2c1cba2269dd90faa7f5c5358eb640d9d65f55fced3)


Compare hg manifests 
  $ diff <(mononoke_admin blobstore -R orig fetch hgmanifest.sha1.c1afe800646ee45232ab5e70c57247b78dbf3899 --quiet) <(mononoke_admin blobstore -R dest fetch hgmanifest.sha1.c1afe800646ee45232ab5e70c57247b78dbf3899 --quiet)  
  $ diff <(mononoke_admin blobstore -R orig fetch hgmanifest.sha1.53b19c5f23977836390e5880ec30fd252a311384 --quiet) <(mononoke_admin blobstore -R dest fetch hgmanifest.sha1.53b19c5f23977836390e5880ec30fd252a311384 --quiet)

Compare filenodes 
  $ diff <(mononoke_admin blobstore -R orig fetch hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9 --quiet) <(mononoke_admin blobstore -R dest fetch hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9 --quiet)  
  $ diff <(mononoke_admin blobstore -R orig fetch hgfilenode.sha1.f9304d84edb8a8ee2d3ce3f9de3ea944c82eba8f --quiet) <(mononoke_admin blobstore -R dest fetch hgfilenode.sha1.f9304d84edb8a8ee2d3ce3f9de3ea944c82eba8f --quiet) 

Compare hg and bonsai changeset
  $ diff <(mononoke_admin fetch  -R orig  --commit-id 53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856) <(mononoke_admin fetch  -R dest  --commit-id 53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856)  
  $ diff <(mononoke_admin fetch  -R orig  --commit-id e20237022b1290d98c3f14049931a8f498c18c53) <(mononoke_admin fetch  -R dest  --commit-id e20237022b1290d98c3f14049931a8f498c18c53)  
