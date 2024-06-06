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
  > hooks_skip_ancestors_of=["main"]
  > CONFIG

  $ register_hook_limit_filesize_global_limit 10 'bypass_pushvar="ALLOW_LARGE_FILES=true"'

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:run_hooks_on_additional_changesets": true
  >   }
  > }
  > EOF

  $ setup_common_hg_configs
  $ cd $TESTTMP

  $ configure dummyssh
  $ enable amend

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ drawdag <<EOF
  > D F           # C/large = file_too_large
  > | |           # E/large = file_too_large
  > C E    Z      # Y/large = file_too_large
  > |/     |
  > B      Y
  > |      |
  > A      X
  > EOF

  $ hg bookmark main -r $A
  $ hg bookmark head_d -r $D
  $ hg bookmark head_f -r $F
  $ hg bookmark head_z -r $Z

blobimport
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
clone
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ enable pushrebase remotenames

fast-forward the bookmark
  $ hg up -q $B
  $ hgedenapi push -r . --to main
  pushing rev 112478962961 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from 426bada5c675 to 112478962961

fast-forward the bookmark over a commit that fails the hook
  $ hg up -q $D
  $ hgedenapi push -r . --to main
  pushing rev 7ff4b7c298ec to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from 112478962961 to 7ff4b7c298ec
  abort: server error: hooks failed:
    limit_filesize for 365e543af2aaf5cca34cf47377a8aee88b5597d45160996bf6434703fca8f8ff: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook, the push will now work
  $ hgedenapi push -r . --to main --pushvar ALLOW_LARGE_FILES=true
  pushing rev 7ff4b7c298ec to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from 112478962961 to 7ff4b7c298ec

attempt a non-fast-forward move, it should fail
  $ hg up -q $F
  $ hgedenapi push -r . --to main
  pushing rev af09fbbc2f05 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  abort: non-fast-forward push to remote bookmark main from 7ff4b7c298ec to af09fbbc2f05
  (add '--force' or set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)
  [255]
specify the pushvar to allow the non-fast-forward move.
  $ hgedenapi push -r . --to main --pushvar NON_FAST_FORWARD=true
  pushing rev af09fbbc2f05 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from 7ff4b7c298ec to af09fbbc2f05
  abort: server error: hooks failed:
    limit_filesize for 9c3ef8778600f6cd1c20c8a098bbb93a4d1b30fee00ff001e37ffff1908c920d: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook too, and it should work
  $ hgedenapi push -r . --to main --pushvar NON_FAST_FORWARD=true --pushvar ALLOW_LARGE_FILES=true
  pushing rev af09fbbc2f05 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from 7ff4b7c298ec to af09fbbc2f05

Noop bookmark-only push doesn't need to bypass hooks to go through.
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) from bookmarks_update_log";
  7
The server side bookmark value can be stable due to data derivation, let's workaround it by reading from local
  $ hgedenapi push -r . --to main --config push.use_local_bookmark_value=True
  pushing rev af09fbbc2f05 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from af09fbbc2f05 to af09fbbc2f05
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) from bookmarks_update_log";
  7

attempt a move to a completely unrelated commit (no common ancestor), with an ancestor that
fails the hook
  $ hg up -q $Z
  $ hgedenapi push -r . --to main --pushvar NON_FAST_FORWARD=true
  pushing rev e3295448b1ef to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from af09fbbc2f05 to e3295448b1ef
  abort: server error: hooks failed:
    limit_filesize for e9bcd19d2580895e76b4e228c3df2ae8d3f2863894ba4d2e9dea3004bdd5abb8: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook, and it should work
  $ hgedenapi push -r . --to main --pushvar NON_FAST_FORWARD=true --pushvar ALLOW_LARGE_FILES=true
  pushing rev e3295448b1ef to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from af09fbbc2f05 to e3295448b1ef

pushing another bookmark to the same commit shouldn't require running that hook
  $ hg up -q $X
  $ hgedenapi push -r . --to other --create
  pushing rev ba2b7fa7166d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  creating remote bookmark other
  $ hg up -q $Z
  $ hgedenapi push -r . --to other
  pushing rev e3295448b1ef to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  moving remote bookmark other from ba2b7fa7166d to e3295448b1ef

but pushing to another commit will run the hook
  $ hg up -q $C
  $ hgedenapi push -r . --to other --pushvar NON_FAST_FORWARD=true
  pushing rev 5e6585e50f1b to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  moving remote bookmark other from e3295448b1ef to 5e6585e50f1b
  abort: server error: hooks failed:
    limit_filesize for 365e543af2aaf5cca34cf47377a8aee88b5597d45160996bf6434703fca8f8ff: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypassing that also works
  $ hgedenapi push -r . --to other --pushvar NON_FAST_FORWARD=true --pushvar ALLOW_LARGE_FILES=true
  pushing rev 5e6585e50f1b to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  moving remote bookmark other from e3295448b1ef to 5e6585e50f1b

we can now extend that bookmark further without a bypass needed
  $ hg up -q $D
  $ hgedenapi push -r . --to other
  pushing rev 7ff4b7c298ec to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  moving remote bookmark other from 5e6585e50f1b to 7ff4b7c298ec

create a new bookmark at this location - it should fail because of the hook
  $ hgedenapi push -r . --to created --create
  pushing rev 7ff4b7c298ec to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark created
  creating remote bookmark created
  abort: failed to create remote bookmark:
    remote server error: hooks failed:
    limit_filesize for 365e543af2aaf5cca34cf47377a8aee88b5597d45160996bf6434703fca8f8ff: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook to allow the creation
  $ hgedenapi push -r . --to created --create --pushvar ALLOW_LARGE_FILES=true
  pushing rev 7ff4b7c298ec to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark created
  creating remote bookmark created

we can, however, create a bookmark at the same location as main
  $ hgedenapi push -r $Z --to main-copy --create
  pushing rev e3295448b1ef to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main-copy
  creating remote bookmark main-copy
