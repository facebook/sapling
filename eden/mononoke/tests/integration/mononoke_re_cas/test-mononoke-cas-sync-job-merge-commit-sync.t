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
  $ drawdag << 'EOF'
  > F        # F/quux = random:30
  > |\       # D/qux  = random:30
  > B D      # C/baz  = random:30
  > | |      # B/bar  = random:30
  > A C      # A/foo  = random:30
  > EOF

  $ hg push -r $B --to master -q --create
  $ hg push -r $D --allow-anon -q
  $ hg push -r $F --to master -q
  $ hg goto $F -q
  $ ls | sort
  bar
  baz
  foo
  quux
  qux

Sync all bookmarks moves (the second move is a merge commit)
  $ with_stripped_logs mononoke_cas_sync repo 0
  Initiating mononoke RE CAS sync command execution for repo repo, repo: repo
  using repo "repo" repoid RepositoryId(0), repo: repo
  syncing log entries [1, 2] ..., repo: repo
  log entry BookmarkUpdateLogEntry * is a creation of bookmark, repo: repo (glob)
  log entries [1, 2] synced (3 commits uploaded, upload stats: uploaded digests: 8, already present digests: 0, uploaded bytes: 1.6 kiB, the largest uploaded blob: 914 B), took overall * sec, repo: repo (glob)
  queue size after processing: 0, repo: repo
  successful sync of entries [1, 2], repo: repo
  Finished mononoke RE CAS sync command execution for repo repo, repo: repo

Verify that all the blobs are in CAS for the merge commit F
  $ with_stripped_logs mononoke_newadmin cas-store --repo-name repo upload --full --hg-id $F
  Upload completed. Upload stats: uploaded digests: 0, already present digests: 6, uploaded bytes: 0 B, the largest uploaded blob: 0 B
