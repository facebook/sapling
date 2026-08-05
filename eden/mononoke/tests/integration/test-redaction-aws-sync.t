# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Test that the AWS sync path is triggered when creating redaction key lists,
and that it fails gracefully when AWS command-line tools are not available.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

  $ cd $TESTTMP

setup repo with testtool_drawdag
  $ testtool_drawdag -R repo --no-default-files --derive-all --print-hg-hashes <<EOF
  > C
  > |
  > A
  > # modify: A "a" "a"
  > # modify: C "secret" "sensitive data"
  > # bookmark: A master_bookmark
  > # bookmark: C other_bookmark
  > EOF
  A=* (glob)
  C=* (glob)

start mononoke
  $ start_and_wait_for_mononoke_server

Test-case: AWS sync discovery failure without `--skip-aws-sync`.
How/setup: Hide the AWS command-line tools from this invocation.
Expectation: The key list is saved locally and the sync prints a warning.

  $ mkdir "$TESTTMP/no-aws-tools"

  $ PATH="$TESTTMP/no-aws-tools" mononoke_admin redaction create-key-list -R repo -i $C secret --main-bookmark master_bookmark --output-file rs_0 2>&1
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: * (glob)
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator
  
  Checking if sync to AWS is required...
    * Warning: Failed to run cloud CLI* (glob)
    * Retry with: monad redaction sync-to-aws (glob)


Test-case: Local-only key list creation with `--skip-aws-sync`.
How/setup: Create the same key list with AWS sync disabled and the restricted tool path.
Expectation: The key list is saved without any AWS sync output.

  $ PATH="$TESTTMP/no-aws-tools" mononoke_admin redaction create-key-list -R repo -i $C secret --main-bookmark master_bookmark --skip-aws-sync --output-file rs_1 2>&1
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: * (glob)
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator

Test-case: The public sync command loads every active key list before syncing.
How/setup: Make the local key list active and hide the AWS command-line tools.
Expectation: The command reads the active list and reports the AWS setup failure.

  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >   "all_redactions": [
  >     {"reason": "T0", "id": "$(cat rs_1)", "enforce": true}
  >   ]
  > }
  > EOF
  $ PATH="$TESTTMP/no-aws-tools" mononoke_admin redaction sync-to-aws > sync-to-aws.out 2>&1
  [1]
  $ grep -F "Found 1 unique key list(s) in the active prod redaction config" sync-to-aws.out
  Found 1 unique key list(s) in the active prod redaction config
  $ grep -F "Read key list " sync-to-aws.out
  Read key list * (1 keys) from prod (glob)
  $ grep -F "AWS sync outcome is unknown for 1 key list(s)" sync-to-aws.out
  AWS sync outcome is unknown for 1 key list(s):
  $ grep -F "Failed: 0" sync-to-aws.out
    Failed: 0
  $ grep -F "AWS operation failed: Failed to run cloud CLI" sync-to-aws.out
  Error: AWS operation failed: Failed to run cloud CLI* (glob)

Test-case: The internal batch operation reports per-list outcomes.
How/setup: Derive a new valid id from a mismatched request, then retry with that id.
Expectation: The mismatch fails without writing, the retry inserts, and a repeat is already present.

  $ KEY_LIST_ID=$(cat rs_1)
  $ KEYS=$(mononoke_admin redaction fetch-key-list -R repo "$KEY_LIST_ID")
  $ MISMATCH_PAYLOAD=$(printf '%s\n' "$KEYS" | jq -Rsc --arg id "$KEY_LIST_ID" '(split("\n") | map(select(length > 0))) as $keys | [{id: $id, keys: ($keys + $keys)}]')
  $ MISMATCH_RESULT=$(mononoke_admin redaction sync-key-lists-from-json --payload "$MISMATCH_PAYLOAD")
  $ echo "$MISMATCH_RESULT"
  MONONOKE_AWS_SYNC_RESULT={"items":[{"status":"failed","id":"*","error":"Content hashes to *, not the requested id *"}]} (glob)
  $ NEW_ID=$(printf '%s\n' "${MISMATCH_RESULT#MONONOKE_AWS_SYNC_RESULT=}" | jq -r '.items[0].error | capture("^Content hashes to (?<id>[^,]+),").id')
  $ INSERT_PAYLOAD=$(printf '%s\n' "$KEYS" | jq -Rsc --arg id "$NEW_ID" '(split("\n") | map(select(length > 0))) as $keys | [{id: $id, keys: ($keys + $keys)}]')
  $ mononoke_admin redaction sync-key-lists-from-json --payload "$INSERT_PAYLOAD"
  MONONOKE_AWS_SYNC_RESULT={"items":[{"status":"inserted","id":"*"}]} (glob)
  $ echo "$INSERT_PAYLOAD" | mononoke_admin redaction sync-key-lists-from-json --payload-stdin
  MONONOKE_AWS_SYNC_RESULT={"items":[{"status":"already_present","id":"*"}]} (glob)
