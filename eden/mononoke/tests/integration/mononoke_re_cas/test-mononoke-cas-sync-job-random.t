# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ export CAS_STORE_PATH="$TESTTMP"
  $ setconfig drawdag.defaultfiles=false

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo
  $ drawdag << EOS
  > F # F/quux = random:30
  > |
  > D # D/qux = random:30
  > |
  > C # C/baz = random:30
  > |
  > B # B/bar = random:30
  > |
  > A # A/foo = random:30
  > EOS

  $ hg goto A -q
  $ hg push -r . --to master -q --create

  $ hg goto B -q
  $ hg push -r . --to master -q

  $ hg goto C -q
  $ hg push -r . --to master -q

  $ hg goto D -q
  $ hg push -r . --to master -q

  $ hg goto F -q
  $ hg push -r . --to other_bookmark -q --create

Check that new entry was added to the sync database. 4 pushes
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) from bookmarks_update_log";
  5

Sync all bookmarks moves and test the "stats" output. This should be stable due to the use of "random", that's why we never expect already present blobs, and uploaded sum should be the same for all runs. Upload should include both bookmarks master and other.
  $ with_stripped_logs mononoke_cas_sync repo 0
  Initiating mononoke RE CAS sync command execution for repo repo, repo: repo
  using repo "repo" repoid RepositoryId(0), repo: repo
  syncing log entries [1, 2, 3, 4, 5] ..., repo: repo
  log entry BookmarkUpdateLogEntry * is a creation of bookmark, repo: repo (glob)
  log entry BookmarkUpdateLogEntry * is a creation of bookmark, repo: repo (glob)
  log entries [1, 2, 3, 4, 5] synced (5 commits uploaded, upload stats: uploaded digests: 10, already present digests: 0, uploaded bytes: 2.8 kiB, the largest uploaded blob: 875 B), took overall * sec, repo: repo (glob)
  queue size after processing: 0, repo: repo
  successful sync of entries [1, 2, 3, 4, 5], repo: repo
  Finished mononoke RE CAS sync command execution for repo repo, repo: repo

Validate that all the blobs are now present in CAS for the commit D
  $ with_stripped_logs mononoke_newadmin cas-store --repo-name repo upload --full --hg-id $D
  Upload completed. Upload stats: uploaded digests: 0, already present digests: 5, uploaded bytes: 0 B, the largest uploaded blob: 0 B

Validate that all the blobs are now present in CAS for the middle commit B
  $ with_stripped_logs mononoke_newadmin cas-store --repo-name repo upload --full --hg-id $B
  Upload completed. Upload stats: uploaded digests: 0, already present digests: 3, uploaded bytes: 0 B, the largest uploaded blob: 0 B

Validate that all the blobs are now present in CAS for the first commit A
  $ with_stripped_logs mononoke_newadmin cas-store --repo-name repo upload --full --hg-id $A
  Upload completed. Upload stats: uploaded digests: 0, already present digests: 2, uploaded bytes: 0 B, the largest uploaded blob: 0 B

Commit F belongs to a different bookmark, validate that the commit is fully uploaded
  $ with_stripped_logs mononoke_newadmin cas-store --repo-name repo upload --full --hg-id $F
  Upload completed. Upload stats: uploaded digests: 0, already present digests: 6, uploaded bytes: 0 B, the largest uploaded blob: 0 B
