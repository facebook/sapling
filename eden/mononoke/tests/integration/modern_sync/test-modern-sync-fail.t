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
  $ mononoke_modern_sync "" sync-once orig dest --start-id 0 2>&1 | grep -v "Uploaded"
  [INFO] Running sync-once loop
  [INFO] [sync{repo=orig}] Opened SourceRepoArgs(Name("orig")) unredacted
  [INFO] [sync{repo=orig}] Starting sync from 0
  [INFO] [sync{repo=orig}] Connecting to https://localhost:$LOCAL_PORT/edenapi/, timeout 300s
  [INFO] [sync{repo=orig}] Established EdenAPI connection
  [INFO] [sync{repo=orig}] Initialized channels
  [INFO] [sync{repo=orig}] Read 2 entries
  [INFO] [sync{repo=orig}] 2 entries left after filtering
  [INFO] [sync{repo=orig}] mononoke_host="*" dogfooding=false (glob)
  [INFO] [sync{repo=orig}] Calculating segments for entry 1, from changeset None to changeset ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856)), to generation 1
  [INFO] [sync{repo=orig}] Done calculating segments for entry 1, from changeset None to changeset ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856)), to generation 1 in *ms (glob)
  [INFO] [sync{repo=orig}] Resuming from latest entry checkpoint 0
  [INFO] [sync{repo=orig}] Skipping 0 batches from entry 1
  [INFO] [sync{repo=orig}] Starting sync of 1 missing commits, 0 were already synced
  [INFO] [sync{repo=orig}] Calculating segments for entry 2, from changeset Some(ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))) to changeset ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8)), approx 4 commit(s)
  [INFO] [sync{repo=orig}] Done calculating segments for entry 2, from changeset Some(ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))) to changeset ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8)), approx 4 commit(s) in *ms (glob)
  [INFO] [sync{repo=orig}] Resuming from latest entry checkpoint 0
  [INFO] [sync{repo=orig}] Skipping 0 batches from entry 2
  [INFO] [sync{repo=orig}] Starting sync of 4 missing commits, 0 were already synced
  .WARN. .sync.repo=orig.. Found error: Trees upload: Expected [0-9]+ responses, got 0, retrying attempt #0 (re)
  .WARN. .sync.repo=orig.. Found error: Trees upload: Expected [0-9]+ responses, got 0, retrying attempt #1 (re)
  .WARN. .sync.repo=orig.. Found error: Trees upload: Expected [0-9]+ responses, got 0, retrying attempt #2 (re)
  .ERROR. .sync.repo=orig.. Failed to upload trees: Trees upload: Expected [0-9]+ responses, got 0 (re)
  .ERROR. .sync.repo=orig.. Trees flush failed: Trees upload: Expected [0-9]+ responses, got 0 (re)
