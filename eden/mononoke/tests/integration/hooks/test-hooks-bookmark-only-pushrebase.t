# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 setup_mononoke_config
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="main"
  > [[bookmarks]]
  > regex=".*"
  > CONFIG

  $ register_hook_limit_filesize_global_limit 10 'bypass_pushvar="ALLOW_LARGE_FILES=true"'

  $ setup_common_hg_configs
  $ cd $TESTTMP

  $ configure dummyssh
  $ enable amend

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ drawdag <<EOF
  > D F           # C/clarge = file_too_large
  > | |           # E/elarge = file_too_large
  > C E    Z      # Y/ylarge = file_too_large
  > |/     |
  > B      Y
  > |      |
  > A      X
  > EOF

  $ hg bookmark main -r $A
  $ hg bookmark x -r $X

blobimport
  $ cd ..
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server

Remove the phase information. See D58415927 for an explanation as to why that is necessary
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

clone
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ enable pushrebase remotenames

fast-forward the bookmark
  $ hg up -q $B
  $ hg push -r . --to main
  pushing rev 112478962961 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  pushrebasing stack (426bada5c675, 112478962961] (1 commit) to remote bookmark main
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark main to 112478962961
  $ wait_for_bookmark_move_away_edenapi repo main "$A"

fast-forward the bookmark over a commit that fails the hook
  $ hg up -q $D
  $ hg push -r . --to main
  pushing rev dc9cf68aa67d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  pushrebasing stack (112478962961, dc9cf68aa67d] (2 commits) to remote bookmark main
  abort: Server error: hooks failed:
    limit_filesize for 31362727a553c6720a19c992d2ffb3de0a2e3e1902a4ce6364ec5a895a82a5ac: File size limit is 10 bytes. You tried to push file clarge that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook, the push will now work
  $ hg push -r . --to main --pushvar ALLOW_LARGE_FILES=true
  pushing rev dc9cf68aa67d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  pushrebasing stack (112478962961, dc9cf68aa67d] (2 commits) to remote bookmark main
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark main to dc9cf68aa67d
  $ wait_for_bookmark_move_away_edenapi repo main "$B"

attempt a non-fast-forward move, it should fail
  $ hg up -q $F
  $ hg push -r . --to main
  pushing rev 24b5f35a12db to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  pushrebasing stack (112478962961, 24b5f35a12db] (2 commits) to remote bookmark main
  abort: Server error: hooks failed:
    limit_filesize for 7cfde619ea7f96e2679b5c8778e93af578be090618a61de57d5f42d6fc30cfec: File size limit is 10 bytes. You tried to push file elarge that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]
specify the pushvar to allow the non-fast-forward move.
  $ hg push -r . --to main --pushvar NON_FAST_FORWARD=true
  pushing rev 24b5f35a12db to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  pushrebasing stack (112478962961, 24b5f35a12db] (2 commits) to remote bookmark main
  abort: Server error: hooks failed:
    limit_filesize for 7cfde619ea7f96e2679b5c8778e93af578be090618a61de57d5f42d6fc30cfec: File size limit is 10 bytes. You tried to push file elarge that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook too, and it should work
  $ hg push -r . --to main --pushvar NON_FAST_FORWARD=true --pushvar ALLOW_LARGE_FILES=true
  pushing rev 24b5f35a12db to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  pushrebasing stack (112478962961, 24b5f35a12db] (2 commits) to remote bookmark main
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark main to 3960ade32ad0
  $ wait_for_bookmark_move_away_edenapi repo main "$D"

Noop bookmark-only push doesn't need to bypass hooks to go through.
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) from bookmarks_update_log";
  5
The server side bookmark value can be stable due to data derivation, let's workaround it by reading from local
  $ hg push -r . --to main --config push.use_local_bookmark_value=True
  pushing rev 3960ade32ad0 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from 3960ade32ad0 to 3960ade32ad0
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) from bookmarks_update_log";
  5

attempt a move to a completely unrelated commit (no common ancestor), with an ancestor that
fails the hook
  $ hg up -q $Z
  $ hg push -r . --to main --pushvar NON_FAST_FORWARD=true
  pushing rev aa22a7abaf7d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  pushrebasing stack (ba2b7fa7166d, aa22a7abaf7d] (2 commits) to remote bookmark main
  abort: Server error: hooks failed:
    limit_filesize for 6644ac858cf67f9d194992b0862ea29f16d8b1fb3a255eba56d928371677dfb4: File size limit is 10 bytes. You tried to push file ylarge that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook, and it should fail: can't push rebase to such a commit
  $ hg push -r . --to main --pushvar NON_FAST_FORWARD=true --pushvar ALLOW_LARGE_FILES=true
  pushing rev aa22a7abaf7d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  pushrebasing stack (ba2b7fa7166d, aa22a7abaf7d] (2 commits) to remote bookmark main
  abort: Server error: invalid request: Pushrebase failed: No common pushrebase root for main, all possible roots: {ChangesetId(Blake2(9e29f9eeba42c4466086108ec239692b0da402e49208848b1cd6dbb9d837ad82))}
  [255]
however, we can create a new bookmark there, bypassing the hook
  $ hg push -r . --to newbookmark --create --pushvar ALLOW_LARGE_FILES=true
  pushing rev aa22a7abaf7d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark newbookmark
  creating remote bookmark newbookmark
then, it's not a pushrebase so we can move main there
  $ hg push -r . --to main --pushvar NON_FAST_FORWARD=true --pushvar ALLOW_LARGE_FILES=true
  pushing rev aa22a7abaf7d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from 3960ade32ad0 to aa22a7abaf7d

pushing another bookmark to the same commit shouldn't require running that hook
  $ hg up -q $X
  $ hg push -r . --to other --create
  pushing rev ba2b7fa7166d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  creating remote bookmark other
  $ hg up -q $Z
  $ hg push -r . --to yet_another --create
  pushing rev aa22a7abaf7d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark yet_another
  creating remote bookmark yet_another
