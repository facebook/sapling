# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ ENABLE_API_WRITES=1 setup_mononoke_config
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="main"
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
  $ hginit_treemanifest repo
  $ cd repo
  $ drawdag <<EOF
  > A X
  > EOF

  $ hg bookmark main -r $A
  $ hg bookmark alternate -r $X

blobimport
  $ cd ..
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
clone
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ enable pushrebase remotenames

make more commits
  $ drawdag <<EOF
  > D F           # C/large = file_too_large
  > | |           # E/large = file_too_large
  > C E    Z      # Y/large = file_too_large
  > |/     |
  > B      Y
  > |      |
  > |      $X
  > $A
  > EOF

fast-forward the bookmark
  $ hg up -q $B
  $ hg push -r . --to main --force
  pushing rev 112478962961 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  moving remote bookmark main from * to 112478962961 (glob)

fast-forward the bookmark over a commit that fails the hook
  $ hg up -q $D
  $ hg push -r . --to main --force
  pushing rev 7ff4b7c298ec to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  edenapi: queue 2 commits for upload
  edenapi: queue 3 files for upload
  edenapi: uploaded 3 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  moving remote bookmark main from * to 7ff4b7c298ec (glob)
  abort: server error: hooks failed:
    limit_filesize for 365e543af2aaf5cca34cf47377a8aee88b5597d45160996bf6434703fca8f8ff: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook, the push will now work
  $ hg push -r . --to main --force --pushvar ALLOW_LARGE_FILES=true
  pushing rev 7ff4b7c298ec to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from * to 7ff4b7c298ec (glob)

attempt a non-fast-forward push over a commit that fails the hook
  $ hg up -q $F
  $ hg push -r . --to main --force
  pushing rev af09fbbc2f05 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  moving remote bookmark main from * to af09fbbc2f05 (glob)
  abort: server error: hooks failed:
    limit_filesize for 9c3ef8778600f6cd1c20c8a098bbb93a4d1b30fee00ff001e37ffff1908c920d: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook, and it should work
  $ hg push -r . --to main --pushvar ALLOW_LARGE_FILES=true --force
  pushing rev af09fbbc2f05 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from * to af09fbbc2f05 (glob)

attempt a move to a completely unrelated commit (no common ancestor), with an ancestor that
fails the hook
  $ hg up -q $Z
  $ hg push -r . --to main --force
  pushing rev e3295448b1ef to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  moving remote bookmark main from * to e3295448b1ef (glob)
  abort: server error: hooks failed:
    limit_filesize for e9bcd19d2580895e76b4e228c3df2ae8d3f2863894ba4d2e9dea3004bdd5abb8: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  [255]

bypass the hook, and it should work
  $ hg push -r . --to main --force --pushvar ALLOW_LARGE_FILES=true
  pushing rev e3295448b1ef to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from * to e3295448b1ef (glob)
