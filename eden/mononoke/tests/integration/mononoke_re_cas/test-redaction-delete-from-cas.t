# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Verify that content redacted after it was synced to CAS can be deleted from
CAS, only once the redaction is enforced.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ export CAS_STORE_PATH="$TESTTMP"
  $ setconfig drawdag.defaultfiles=false

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo

B adds "secret" and C replaces its content, so only B's version gets redacted.
We use random:30 content because the test CAS backend is shared, so
deterministic content may already exist from prior runs.
  $ drawdag << EOS
  > C # C/secret = random:30
  > |
  > B # B/secret = random:30
  > |
  > A # A/public/readme = random:30
  > EOS

  $ hg goto $A -q
  $ hg push -r . --to master_bookmark -q --create
  $ hg goto $B -q
  $ hg push -r . --to master_bookmark -q
  $ hg goto $C -q
  $ hg push -r . --to master_bookmark -q

Sync everything to CAS before redacting, as the live sync does at land time.
  $ mononoke_cas_sync repo 0 2>&1 | grep -E 'synced|successful sync'
  [INFO] [execute{repo=repo}] log entries [1, 2, 3] synced (3 commits uploaded, upload stats: uploaded digests: 7, already present digests: 0, uploaded bytes: *, the largest uploaded blob: *), took overall * sec* (glob)
  [INFO] [execute{repo=repo}] successful sync of entries [1, 2, 3]

Redact the version of "secret" introduced in B, in log-only mode first.
  $ cd "$TESTTMP"
  $ mononoke_admin redaction create-key-list -R repo -i $B secret --main-bookmark master_bookmark --output-file rs_0 --skip-aws-sync
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: * (glob)
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator

  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >  "all_redactions": [
  >    {"reason": "T0", "id": "$(cat rs_0)", "enforce": false}
  >  ]
  > }
  > EOF

While the key list is log-only, CAS sync could upload the content again, so
deleting is refused.
  $ mononoke_admin redaction delete-from-cas -R repo $(cat rs_0)
  Error: Refusing to delete from CAS: key lists must be enforced in the redaction config first, but these are not: * (glob)
  [1]

  $ mononoke_admin redaction delete-from-cas -R repo --all-enforced
  Error: The redaction config contains no enforced key lists
  [1]

Enforce the redaction.
  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >  "all_redactions": [
  >    {"reason": "T0", "id": "$(cat rs_0)", "enforce": true}
  >  ]
  > }
  > EOF

A dry run reports the redacted file as present and deletes nothing.
  $ mononoke_admin redaction delete-from-cas -R repo $(cat rs_0) --dry-run
  Found 1 redacted files in 1 key lists
  source-control-testing: 1 of 1 redacted files are present in CAS
    *:30 (glob)

Delete it. It is still present, so the dry run deleted nothing.
  $ mononoke_admin redaction delete-from-cas -R repo $(cat rs_0)
  Found 1 redacted files in 1 key lists
  source-control-testing: 1 of 1 redacted files are present in CAS
    *:30 (glob)
  source-control-testing: deleted 1 redacted files and verified they are absent

Running it again finds nothing to delete.
  $ mononoke_admin redaction delete-from-cas -R repo --all-enforced
  Found 1 redacted files in 1 key lists
  source-control-testing: 0 of 1 redacted files are present in CAS

Everything else in B is still in CAS, and the redacted file is not uploaded
again because the redaction is enforced.
  $ mononoke_admin cas-store --repo-name repo upload --full -i $B
  [WARN] Skipped 1 redacted files for changeset * (glob)
  [INFO] Upload completed. Upload stats: uploaded digests: 0, already present digests: 3, uploaded bytes: 0 B, the largest uploaded blob: 0 B

The newer, unredacted version of "secret" in C is untouched.
  $ mononoke_admin cas-store --repo-name repo upload --full -i $C -p secret
  [INFO] Upload completed. Upload stats: uploaded digests: 0, already present digests: 1, uploaded bytes: 0 B, the largest uploaded blob: 0 B
