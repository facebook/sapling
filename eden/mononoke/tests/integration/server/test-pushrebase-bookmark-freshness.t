# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Keep the warm bookmark cache on its initial value for the duration of the test.

  $ export ENABLE_BOOKMARK_CACHE=1
  $ setconfig push.edenapi=true
  $ setup_common_config
  $ merge_just_knobs <<'EOF'
  > {"ints":{"scm/mononoke:warm_bookmark_cache_poll_interval_ms":600000}}
  > EOF

Create A and B, but initialize the bookmark and its warm cache at A.

  $ cd "$TESTTMP"
  $ testtool_drawdag -R repo --print-hg-hashes <<'EOF'
  > B
  > |
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  $ start_and_wait_for_mononoke_server

  $ hg clone -q mono:repo client --noupdate
  $ cd client

Move the primary bookmark to B while the warm cache remains at A.

  $ hg debugapi -e setbookmark -i "'master_bookmark'" -i "'$B'" -i "'$A'"
  {"data": {"Ok": None}}
  $ hg debugapi -e bookmarks -i "['master_bookmark']" -i "'MaybeStale'"
  {"master_bookmark": "20ca2a4749a439b459125ef0f6a4f26e88ee7538"}
  $ hg debugapi -e bookmarks -i "['master_bookmark']" -i "'MostRecent'"
  {"master_bookmark": "80521a640a0c8f51dcc128c2658b224d595840ac"}

Recreate the state after a fresh pull: B is present and recorded as the local
remote bookmark, so it is public. Then create C on top.

  $ hg -q debugsh -c "repo.pull(source='default', bookmarknames=('master_bookmark',), remotebookmarks={'master_bookmark': s.node.bin('$B')})"
  $ hg log -r remote/master_bookmark -T '{node} {phase}\n'
  80521a640a0c8f51dcc128c2658b224d595840ac public
  $ hg update -q remote/master_bookmark
  $ echo C > C
  $ hg add C
  $ hg commit -qm C

Fresh bookmark reads are enabled by default, so only C is part of the stack.

  $ hg push -r . --to master_bookmark
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (80521a640a0c, *] (1 commit) to remote bookmark master_bookmark (glob)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to * (glob)
