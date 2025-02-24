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
  Established EdenAPI connection
  Initialized channels
  Calculating segments for entry 1
  Skipping 0 commits, starting sync of 1 commits 
  Found error: Trees upload: Expected 1 responses, got 0, retrying attempt #0
  Found error: Trees upload: Expected 1 responses, got 0, retrying attempt #1
  Found error: Trees upload: Expected 1 responses, got 0, retrying attempt #2
  Error processing files/trees: Failed to upload trees: HgManifestId(HgNodeHash(Sha1(c1afe800646ee45232ab5e70c57247b78dbf3899)))
  
  Caused by:
      Trees upload: Expected 1 responses, got 0
  Execution error: Error while waiting for commit to be synced Error processing changesets: Files/trees error received. Winding down changesets sender.
  
  Caused by:
      0: Failed to upload trees: HgManifestId(HgNodeHash(Sha1(c1afe800646ee45232ab5e70c57247b78dbf3899)))
      1: Trees upload: Expected 1 responses, got 0
  Error: Execution failed
  [1]
