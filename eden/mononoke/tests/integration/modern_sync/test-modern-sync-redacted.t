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
  > # bookmark: C master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:orig orig
  $ cd orig
  $ hg up -q $A
Create another commit that has other content we can redact
  $ echo c > c
  $ hg ci -A -q -m 'add c'
  $ hg bookmark other_bookmark -r tip

Redact file 'C' in commit '477211daba9d'
  $ mononoke_admin redaction create-key-list -R orig -i $C C --main-bookmark master_bookmark --force --output-file rs_0
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  Redacted content in main bookmark: C content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  Creating key list despite 1 files being redacted in the main bookmark (master_bookmark) (--force)
  Redaction saved as: 0ad6b3f81ec02c29eff93718a38503cdd45c95ffbf12b0be83a149f039d692c8
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator

  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >  "all_redactions": [
  >    {"reason": "T0", "id": "$(cat rs_0)", "enforce": true}
  >  ]
  > }
  > EOF
  $ rm rs_0

The files should now be marked as redacted
  $ mononoke_admin redaction list -R orig -i $C
  Searching for redacted paths in e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  Found 1 redacted paths
  T0                  : C

Restart mononoke server to pick up the redaction config without relying on timing
  $ termandwait $MONONOKE_PID
  $ start_and_wait_for_mononoke_server

Sync all bookmarks moves
  $ RUST_LOG="INFO,http_client::handler::streaming=OFF,mononoke_modern_sync_job::sender::edenapi::retry=OFF,mononoke_modern_sync_job::sender::manager=OFF,mononoke_modern_sync_job::sender::manager::content=WARN" \
  > mononoke_modern_sync "" sync-once orig dest --start-id 0
  [INFO] Running sync-once loop
  [INFO] [sync{repo=orig}] Opened SourceRepoArgs(Name("orig")) unredacted
  [INFO] [sync{repo=orig}] Starting sync from 0
  [INFO] [sync{repo=orig}] Connecting to https://localhost:$LOCAL_PORT/edenapi/, timeout 300s
  [INFO] [sync{repo=orig}] Established EdenAPI connection
  [INFO] [sync{repo=orig}] Initialized channels
  [INFO] [sync{repo=orig}] Read 1 entries
  [INFO] [sync{repo=orig}] 1 entries left after filtering
  [INFO] [sync{repo=orig}] mononoke_host="*" dogfooding=false (glob)
  [INFO] [sync{repo=orig}] Calculating segments for entry 1, from changeset None to changeset ChangesetId(Blake2(e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2)), to generation 3
  [INFO] [sync{repo=orig}] Done calculating segments for entry 1, from changeset None to changeset ChangesetId(Blake2(e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2)), to generation 3 in *ms (glob)
  [INFO] [sync{repo=orig}] Resuming from latest entry checkpoint 0
  [INFO] [sync{repo=orig}] Skipping 0 batches from entry 1
  [INFO] [sync{repo=orig}] Starting sync of 3 missing commits, 0 were already synced
  [ERROR] [sync{repo=orig}] Error processing content: collecting contents entries
  
  Caused by:
      server responded 500 Internal Server Error for https://localhost:$LOCAL_PORT/edenapi/dest/upload/file/content_id/896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d?content_size=1: {"message":"internal error: The blob content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d is censored.\n Task/Sev: T0: The blob content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d is censored.\n Task/Sev: T0: The blob content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d is censored.\n Task/Sev: T0","request_id":"*"}. Headers: { (glob)
          "x-request-id": "*", (glob)
          "content-type": "application/json",
          "x-load": "*", (glob)
          "server": "edenapi_server",
          "x-mononoke-host": "*", (glob)
          "content-length": "406",
          "date": "*", (glob)
      }
  [ERROR] [sync{repo=orig}] Error processing content: collecting contents entries
  
  Caused by:
      server responded 500 Internal Server Error for https://localhost:$LOCAL_PORT/edenapi/dest/upload/file/content_id/896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d?content_size=1: {"message":"internal error: The blob content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d is censored.\n Task/Sev: T0: The blob content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d is censored.\n Task/Sev: T0: The blob content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d is censored.\n Task/Sev: T0","request_id":"*"}. Headers: { (glob)
          "x-request-id": "*", (glob)
          "content-type": "application/json",
          "x-load": "*", (glob)
          "server": "edenapi_server",
          "x-mononoke-host": "*", (glob)
          "content-length": "406",
          "date": "*", (glob)
      }


Removing the redaction config should allow the sync to succeed. That is the scenario we use in the initial AWS sync
  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >  "all_redactions": [
  >  ]
  > }
  > EOF

The files should not be marked as redacted
  $ mononoke_admin redaction list -R orig -i $C
  Searching for redacted paths in e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  Found 0 redacted paths

Restart mononoke server to pick up the redaction config without relying on timing
  $ termandwait $MONONOKE_PID
  $ start_and_wait_for_mononoke_server

Sync all bookmarks moves
  $ mononoke_modern_sync "" sync-once orig dest --start-id 0 2>&1 | grep -v "Uploaded"
  [INFO] Running sync-once loop
  [INFO] [sync{repo=orig}] Opened SourceRepoArgs(Name("orig")) unredacted
  [INFO] [sync{repo=orig}] Starting sync from 0
  [INFO] [sync{repo=orig}] Connecting to https://localhost:$LOCAL_PORT/edenapi/, timeout 300s
  [INFO] [sync{repo=orig}] Established EdenAPI connection
  [INFO] [sync{repo=orig}] Initialized channels
  [INFO] [sync{repo=orig}] Read 1 entries
  [INFO] [sync{repo=orig}] 1 entries left after filtering
  [INFO] [sync{repo=orig}] mononoke_host="*" dogfooding=false (glob)
  [INFO] [sync{repo=orig}] Calculating segments for entry 1, from changeset None to changeset ChangesetId(Blake2(e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2)), to generation 3
  [INFO] [sync{repo=orig}] Done calculating segments for entry 1, from changeset None to changeset ChangesetId(Blake2(e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2)), to generation 3 in *ms (glob)
  [INFO] [sync{repo=orig}] Resuming from latest entry checkpoint 0
  [INFO] [sync{repo=orig}] Skipping 0 batches from entry 1
  [INFO] [sync{repo=orig}] Starting sync of 3 missing commits, 0 were already synced
  [INFO] [sync{repo=orig}] Setting checkpoint from entry 1 to 0
  [INFO] [sync{repo=orig}] Setting bookmark master_bookmark from None to Some(HgChangesetId(HgNodeHash(Sha1(d3b399ca8757acdb81c3681b052eb978db6768d8))))
  [INFO] [sync{repo=orig}] Moved bookmark with result SetBookmarkResponse { data: Ok(()) }
  [INFO] [sync{repo=orig}] Marking entry 1 as done
