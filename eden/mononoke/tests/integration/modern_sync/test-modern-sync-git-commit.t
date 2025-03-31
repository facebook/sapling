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

  $ testtool_drawdag -R orig --derive-all --print-hg-hashes <<EOF
  > A-B-C
  > # default_files: false
  > # bookmark: C master_bookmark
  > # author: C "anybody <anybody@fb.com>"
  > # committer: C "Foo Bar <fb@meta.com>"
  > # committer_date: C "2023-05-23T11:15:49-07:00"
  > EOF
  A=5dce5fe8bca8dd081ee2f8c35e50ee70a916016b
  B=b98d3e9b109b55f1efbd52b59d030cda50c71bb0
  C=0cbc65aa8c36273308c2f1aa0f7d81188129f45e

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:orig orig
  $ cd orig
  $ hg up -q $C
  $ mononoke_admin fetch -R orig -i $C --json | jq .
  {
    "changeset_id": "6fd02f115a2c76bc502c5a5bb4f67a48dd92e8e9db70d3c83ef7d6a5bf2dd898",
    "parents": [
      "2c502b961490c44a5809856d6b6f3d75c0db038bc8067573db3a7355df26b377"
    ],
    "author": "anybody <anybody@fb.com>",
    "author_date": "1970-01-01T00:00:00Z",
    "committer": "Foo Bar <fb@meta.com>",
    "committer_date": "2023-05-23T11:15:49-07:00",
    "message": "C",
    "hg_extra": {},
    "file_changes": {}
  }
  $ cd ..

Sync all bookmarks moves
  $ quiet mononoke_modern_sync "" sync-once orig dest --start-id 0

Clone and verify the destination repo
  $ cd ..
  $ hg clone -q mono:dest dest
  $ cd dest
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  @  0cbc65aa8c36273308c2f1aa0f7d81188129f45e C
  │
  o  b98d3e9b109b55f1efbd52b59d030cda50c71bb0 B
  │
  o  5dce5fe8bca8dd081ee2f8c35e50ee70a916016b A
  

$ Verify that the destination repo has the same commit hashes as the source repo
  $ hg clone -q mono:dest dest
  $ cd dest
  $ hg up -q $C
  $ mononoke_admin fetch -R dest -i $C --json | jq .
  {
    "changeset_id": "6fd02f115a2c76bc502c5a5bb4f67a48dd92e8e9db70d3c83ef7d6a5bf2dd898",
    "parents": [
      "2c502b961490c44a5809856d6b6f3d75c0db038bc8067573db3a7355df26b377"
    ],
    "author": "anybody <anybody@fb.com>",
    "author_date": "1970-01-01T00:00:00Z",
    "committer": "Foo Bar <fb@meta.com>",
    "committer_date": "2023-05-23T11:15:49-07:00",
    "message": "C",
    "hg_extra": {},
    "file_changes": {}
  }
