# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ export CAS_STORE_PATH="$TESTTMP"

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo
  $ drawdag << EOS
  > D # D/bar = zero\nuno\ntwo\n
  > |
  > C # C/bar = zero\none\ntwo\n (renamed from foo)
  > |
  > B # B/foo = one\ntwo\n
  > |
  > A # A/foo = one\n
  > EOS

  $ hg goto A -q
  $ hg push -r . --to master -q --create

  $ hg goto B -q
  $ hg push -r . --to master -q

  $ hg goto C -q
  $ hg push -r . --to master -q

  $ hg goto D -q
  $ hg push -r . --to master -q

Check that new entry was added to the sync database. 4 pushes
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) from bookmarks_update_log";
  4

Sync all bookmarks moves
  $ with_stripped_logs mononoke_cas_sync repo 0
  Initiating mononoke RE CAS sync command execution for repo repo, repo: repo
  using repo "repo" repoid RepositoryId(0), repo: repo
  syncing log entries [1, 2, 3, 4] ..., repo: repo
  log entry BookmarkUpdateLogEntry * is a creation of bookmark, repo: repo (glob)
  log entries [1, 2, 3, 4] synced *, repo: repo (glob)
  queue size after processing: 0, repo: repo
  successful sync of entries [1, 2, 3, 4], repo: repo
  Finished mononoke RE CAS sync command execution for repo repo, repo: repo

Validate that the whole working copy for the top commit D is already present in CAS, nothing should be uploaded if incremental sync is correct.
All trees and blobs should be present!
  $ with_stripped_logs mononoke_newadmin cas-store --repo-name repo upload --full --hg-id $D
  Upload completed. Upload stats: uploaded digests: 0, already present digests: 6, uploaded bytes: 0 B, the largest uploaded blob: 0 B

Validate the same for a middle commit B
  $ with_stripped_logs mononoke_newadmin cas-store --repo-name repo upload --full --hg-id $B
  Upload completed. Upload stats: uploaded digests: 0, already present digests: 4, uploaded bytes: 0 B, the largest uploaded blob: 0 B
