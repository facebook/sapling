# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Verify that CAS sync skips uploading content under restricted paths.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ export CAS_STORE_PATH="$TESTTMP"
  $ setconfig drawdag.defaultfiles=false

Configure restricted paths: "restricted" directory is restricted.
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<EOF
  > [restricted_paths_config]
  > path_acls = { "restricted" = "SERVICE_IDENTITY:restricted_acl" }
  > [restricted_paths_config.manifest_id_store_config]
  > use_manifest_id_cache = false
  > cache_update_interval_ms = 1000
  > EOF

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo

Create commits with files under both restricted and unrestricted paths.
  $ drawdag << EOS
  > B # B/restricted/secret = random:30
  > |
  > A # A/public/readme = random:30
  > EOS

  $ hg goto A -q
  $ hg push -r . --to master_bookmark -q --create

  $ hg goto B -q
  $ hg push -r . --to master_bookmark -q

Sync all bookmark moves. The sync should detect restricted path roots and
filter out entries under "restricted/". Without filtering, the sync would
upload 6 digests (3 per commit). With filtering, the restricted tree and
file are skipped from commit B, resulting in 4 digests.
  $ mononoke_cas_sync repo 0
  [INFO] [execute{repo=repo}] Initiating mononoke RE CAS sync command execution
  [INFO] [execute{repo=repo}] using repo "repo" repoid RepositoryId(0)
  [INFO] [execute{repo=repo}] syncing log entries [1, 2] ...
  [INFO] [execute{repo=repo}] log entry BookmarkUpdateLogEntry * is a creation of bookmark (glob)
  [INFO] [execute{repo=repo}] Found 1 restricted path roots for changeset *: [NonRootMPath("restricted")] (glob)
  [INFO] [execute{repo=repo}] Filtered out 2 of 3 entries under restricted paths for changeset * (glob)
  [INFO] [execute{repo=repo}] log entries [1, 2] synced (2 commits uploaded, upload stats: uploaded digests: *, already present digests: *, uploaded bytes: *, the largest uploaded blob: *), took overall * sec, derivation checks took * sec (glob)
  [INFO] [execute{repo=repo}] queue size after processing: 0
  [INFO] [execute{repo=repo}] successful sync of entries [1, 2]
  [INFO] [execute{repo=repo}] Finished mononoke RE CAS sync command execution for repo repo

Validate unrestricted content for commit A is fully present in CAS.
  $ mononoke_admin cas-store --repo-name repo upload --full -i $A
  [INFO] Upload completed. Upload stats: uploaded digests: *, already present digests: *, uploaded bytes: *, the largest uploaded blob: * (glob)

Validate that restricted content for commit B was NOT uploaded to CAS.
The sync filtered the restricted tree + file, so uploading the restricted
subtree should find nothing already present. We use random:30 content because
the test CAS backend (RemoteExecutionCasdClient) is shared -- deterministic
content may already exist from prior runs.
  $ mononoke_admin cas-store --repo-name repo upload --full -i $B -p restricted
  [INFO] Upload completed. Upload stats: uploaded digests: 2, already present digests: 0, uploaded bytes: 240 B, the largest uploaded blob: 210 B
