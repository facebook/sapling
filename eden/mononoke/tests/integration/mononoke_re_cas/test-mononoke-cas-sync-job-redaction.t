# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Verify that CAS sync skips file content that was redacted after it landed,
instead of failing, and still uploads everything else.

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

Redact the version of "secret" introduced in B, as if it was redacted after
landing. It is no longer reachable from master_bookmark, so no --force needed.
  $ cd "$TESTTMP"
  $ mononoke_admin redaction create-key-list -R repo -i $B secret --main-bookmark master_bookmark --output-file rs_0 --skip-aws-sync
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: * (glob)
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator

  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >  "all_redactions": [
  >    {"reason": "T0", "id": "$(cat rs_0)", "enforce": true}
  >  ]
  > }
  > EOF
  $ rm rs_0

  $ mononoke_admin redaction list -R repo -i $B
  Searching for redacted paths in * (glob)
  Found 1 redacted paths
  T0                  : secret

Sync all bookmark moves. Without redaction this uploads 7 digests (A: root
tree, "public" tree, public/readme; B: root tree, secret; C: root tree,
secret). The redacted version of "secret" is skipped, so the sync succeeds
with 6.
  $ mononoke_cas_sync repo 0
  [INFO] [execute{repo=repo}] Initiating mononoke RE CAS sync command execution
  [INFO] [execute{repo=repo}] using repo "repo" repoid RepositoryId(0) and CAS use case "source-control-testing"
  [INFO] [execute{repo=repo}] syncing log entries [1, 2, 3] ...
  [INFO] [execute{repo=repo}] log entry BookmarkUpdateLogEntry * is a creation of bookmark (glob)
  [WARN] [execute{repo=repo}] Skipped 1 redacted files for changeset * (glob)
  [INFO] [execute{repo=repo}] log entries [1, 2, 3] synced (3 commits uploaded, upload stats: uploaded digests: 6, already present digests: 0, uploaded bytes: *, the largest uploaded blob: *), took overall * sec, derivation checks took * sec (glob)
  [INFO] [execute{repo=repo}] queue size after processing: 0
  [INFO] [execute{repo=repo}] successful sync of entries [1, 2, 3]
  [INFO] [execute{repo=repo}] Finished mononoke RE CAS sync command execution for repo repo

Commit A has no redacted content and is fully present in CAS.
  $ mononoke_admin cas-store --repo-name repo upload --full -i $A
  [INFO] Upload completed. Upload stats: uploaded digests: 0, already present digests: 3, uploaded bytes: 0 B, the largest uploaded blob: 0 B

A full walk of commit B finds everything else present and skips the redacted
file again rather than failing.
  $ mononoke_admin cas-store --repo-name repo upload --full -i $B
  [WARN] Skipped 1 redacted files for changeset * (glob)
  [INFO] Upload completed. Upload stats: uploaded digests: 0, already present digests: 3, uploaded bytes: 0 B, the largest uploaded blob: 0 B

The newer, unredacted version of "secret" in C was uploaded.
  $ mononoke_admin cas-store --repo-name repo upload --full -i $C -p secret
  [INFO] Upload completed. Upload stats: uploaded digests: 0, already present digests: 1, uploaded bytes: 0 B, the largest uploaded blob: 0 B
