# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ export CAS_STORE_PATH="$TESTTMP"

Given commits created by Mononoke without Hg changesets, with the main bookmark at C
  $ testtool_drawdag -R repo <<'EOF'
  > A-B-C-D
  > # bookmark: C master_bookmark
  > EOF
  A=* (glob)
  B=* (glob)
  C=* (glob)
  D=* (glob)
  $ mononoke_admin derived-data -R repo exists -T hgchangesets -i $A -i $B -i $C -i $D
  Not Derived: * (glob)
  Not Derived: * (glob)
  Not Derived: * (glob)
  Not Derived: * (glob)

When the CAS sync job syncs the bookmark creation
Then each commit uploads its new file and root tree
  $ mononoke_cas_sync repo 0
  [INFO] [execute{repo=repo}] Initiating mononoke RE CAS sync command execution
  [INFO] [execute{repo=repo}] using repo "repo" repoid RepositoryId(0) and CAS use case "source-control-testing"
  [INFO] [execute{repo=repo}] syncing log entries [1] ...
  [INFO] [execute{repo=repo}] log entry BookmarkUpdateLogEntry { id: 1, repo_id: * } is a creation of bookmark (glob)
  [INFO] [execute{repo=repo}] log entries [1] synced (3 commits uploaded, upload stats: uploaded digests: 6, already present digests: 0, uploaded bytes: *, the largest uploaded blob: *), took overall * sec (glob)
  [INFO] [execute{repo=repo}] queue size after processing: 0
  [INFO] [execute{repo=repo}] successful sync of entries [1]
  [INFO] [execute{repo=repo}] Finished mononoke RE CAS sync command execution for repo repo

And the whole working copy of C is present in CAS
  $ mononoke_admin cas-store --repo-name repo upload --full -i $C
  [INFO] Upload completed. Upload stats: uploaded digests: 0, already present digests: 4, uploaded bytes: 0 B, the largest uploaded blob: 0 B

When uploading D, which still has no Hg changeset, with the admin tool
(stats depend on what the CAS backend already holds, because this upload looks digests up first)
  $ mononoke_admin derived-data -R repo exists -T hgchangesets -i $D
  Not Derived: * (glob)
  $ mononoke_admin cas-store --repo-name repo upload -i $D
  [INFO] Upload completed. Upload stats: * (glob)

Then the whole working copy of D is present in CAS
  $ mononoke_admin cas-store --repo-name repo upload --full -i $D
  [INFO] Upload completed. Upload stats: uploaded digests: 0, already present digests: 5, uploaded bytes: 0 B, the largest uploaded blob: 0 B
