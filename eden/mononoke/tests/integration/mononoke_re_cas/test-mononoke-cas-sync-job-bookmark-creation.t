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
  $ hgmn_init repo
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

  $ sl goto D -q
  $ sl push -r . --to master -q --create


Sync all bookmark moves and test the "stats" output. For now, incorrectly, only 1 commit will be synced for the initial boomark creation move, instead of the range [from=None, to=rev]
  $ with_stripped_logs mononoke_cas_sync repo 0 | grep stats
  log entries [1] synced (1 commits uploaded, upload stats: uploaded digests: 2, already present digests: 0, uploaded bytes: 747 B, the largest uploaded blob: 717 B), took overall * sec, repo: repo (glob)
