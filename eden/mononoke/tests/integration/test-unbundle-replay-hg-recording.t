# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Clean up state out of Scuba logs

  $ unset SMC_TIERS TW_TASK_ID TW_CANARY_ID TW_JOB_CLUSTER TW_JOB_USER TW_JOB_NAME

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false
  $ GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark ENABLE_PRESERVE_BUNDLE2=1 BLOB_TYPE="blob_files" quiet default_setup

Set up script to output the raw bundle. This doesn't look at its arguments at all

  $ BUNDLE_PATH="$(realpath "${TESTTMP}/bundle")"
  $ BUNDLE_HELPER="$(realpath "${TESTTMP}/bundle_helper.sh")"
  $ cat > "$BUNDLE_HELPER" <<EOF
  > #!/bin/bash
  > cat "$BUNDLE_PATH"
  > EOF
  $ chmod +x "$BUNDLE_HELPER"

Pushrebase commit

  $ hg up -q "min(all())"
  $ echo "foo" > foo
  $ hg commit -Aqm "add foo"
  $ echo "bar" > bar
  $ hg commit -Aqm "add bar"
  $ hg log -r 0::. -T '{node}\n'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  4afe8a7fa62cf8320c8c11191d4dfdaaed9fb28b
  461b7a0d0ccf85d1168e2ae1be2a85af1ad62826
  $ quiet hgmn push -r . --to master_bookmark
  $ hg log -r ::master_bookmark -T '{node}\n'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  112478962961147124edd43549aedd1a335e44bf
  26805aba1e600a82e93661149f2313866a221a7b
  cbab85d064b0fbdd3e9caa125f8eeac0fb5acf6a
  7a8f33ce453248a6f5cc4747002e931c77234fbd

Check bookmark history

  $ mononoke_admin bookmarks log -c hg master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  * (master_bookmark) 7a8f33ce453248a6f5cc4747002e931c77234fbd pushrebase * (glob)
  * (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

Export the bundle so we can replay it as it if were coming from hg, through the $BUNDLE_HELPER

  $ quiet mononoke_newadmin hg-sync -R repo fetch-bundle 2 --output "$BUNDLE_PATH"

Blow everything away: we're going to re-do the push from scratch, in a new repo.

  $ killandwait "$MONONOKE_PID"
  $ rm -rf "$TESTTMP/mononoke-config" "$TESTTMP/monsql" "$TESTTMP/blobstore"
  $ GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark BLOB_TYPE="blob_files" quiet default_setup

Replay the push. This will fail because the entry does not exist (we need run this once to create the schema).

  $ unbundle_replay hg-recording "$BUNDLE_HELPER" 1
  * Loading repository: repo (id = 0) (glob)
  * Execution error: Entry with id 1 does not exist (glob)
  Error: Execution failed
  [1]

Insert the entry. Note that in tests, the commit timestamp will always be zero.

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" << EOS
  > INSERT INTO pushrebaserecording(repo_id, onto, ontorev, bundlehandle, timestamps, ordered_added_revs, duration_ms) VALUES (
  >   0,
  >   'master_bookmark',
  >   '26805aba1e600a82e93661149f2313866a221a7b',
  >   'handle123',
  >   '{"4afe8a7fa62cf8320c8c11191d4dfdaaed9fb28b": [0.0, 0], "461b7a0d0ccf85d1168e2ae1be2a85af1ad62826": [0.0, 0]}',
  >   '["cbab85d064b0fbdd3e9caa125f8eeac0fb5acf6a", "7a8f33ce453248a6f5cc4747002e931c77234fbd"]',
  >   123
  > );
  > EOS

Replay the push. It will succeed now

  $ quiet unbundle_replay --run-hooks --scuba-log-file "$TESTTMP/scuba.json" hg-recording "$BUNDLE_HELPER" 1

Check history again. We're back to where we were:

  $ mononoke_admin bookmarks log -c hg master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  * (master_bookmark) 7a8f33ce453248a6f5cc4747002e931c77234fbd pushrebase * (glob)
  * (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

  $ format_single_scuba_sample < $TESTTMP/scuba.json
  {
    "int": {
      "age_s": *, (glob)
      "hooks_execution_time_us": *, (glob)
      "pushrebase_completion_time_us": *, (glob)
      "pushrebase_recorded_time_us": 123000,
      "seq": *, (glob)
      "time": *, (glob)
      "unbundle_changeset_count": 2,
      "unbundle_completion_time_us": * (glob)
      "unbundle_file_count": 2
    },
    "normal": {
      "bookmark": "master_bookmark",
      "from_cs_id": "c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd",
      "to_cs_id": "604bc07f395768cd320516a640bef6a1af75d13b4214d44ae3faa2a36f1203bb"
    }
  }
