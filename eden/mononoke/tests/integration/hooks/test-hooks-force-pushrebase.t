# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ setup_mononoke_config
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="master_bookmark"
  > CONFIG

  $ register_hook_limit_filesize_global_limit 10 'bypass_pushvar="ALLOW_LARGE_FILES=true"'

  $ setup_common_hg_configs
  $ setconfig remotenames.selectivepulldefault=master_bookmark,alternate

  $ cd $TESTTMP

  $ configure dummyssh
  $ enable amend

setup repo
  $ testtool_drawdag -R repo <<EOF
  > A X
  > # bookmark: A master_bookmark
  > # bookmark: X alternate
  > EOF
  A=* (glob)
  X=* (glob)

start mononoke
  $ start_and_wait_for_mononoke_server
clone
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ hg pull -q
  $ enable pushrebase

make more commits
  $ A_HASH=$(hg log -r master_bookmark -T '{node}')
  $ X_HASH=$(hg log -r alternate -T '{node}')
  $ drawdag <<EOF
  > D F           # C/large = file_too_large
  > | |           # E/large = file_too_large
  > C E    Z      # Y/large = file_too_large
  > |/     |
  > B      Y
  > |      |
  > |      \$X_HASH
  > \$A_HASH
  > EOF

fast-forward the bookmark
  $ hg up -q $B
  $ hg push -r . --to master_bookmark --force
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  moving remote bookmark master_bookmark from * to * (glob)

fast-forward the bookmark over a commit that fails the hook
  $ hg up -q $D
  $ hg push -r . --to master_bookmark --force
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 2 commits for upload
  edenapi: queue 3 files for upload
  edenapi: uploaded 3 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  moving remote bookmark master_bookmark from * to * (glob)
  abort: server error: hooks failed:
    limit_filesize for *: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes, 1.40x the limit). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions. (glob)
  [255]

bypass the hook, the push will now work
  $ hg push -r . --to master_bookmark --force --pushvar ALLOW_LARGE_FILES=true
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  moving remote bookmark master_bookmark from * to * (glob)

attempt a non-fast-forward push over a commit that fails the hook
  $ hg up -q $F
  $ hg push -r . --to master_bookmark --force
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  moving remote bookmark master_bookmark from * to * (glob)
  abort: server error: hooks failed:
    limit_filesize for *: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes, 1.40x the limit). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions. (glob)
  [255]

bypass the hook, and it should work
  $ hg push -r . --to master_bookmark --pushvar ALLOW_LARGE_FILES=true --force
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  moving remote bookmark master_bookmark from * to * (glob)

attempt a move to a completely unrelated commit (no common ancestor), with an ancestor that
fails the hook
  $ hg up -q $Z
  $ hg push -r . --to master_bookmark --force
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  moving remote bookmark master_bookmark from * to * (glob)
  abort: server error: hooks failed:
    limit_filesize for *: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes, 1.40x the limit). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions. (glob)
  [255]

bypass the hook, and it should work
  $ hg push -r . --to master_bookmark --force --pushvar ALLOW_LARGE_FILES=true
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  moving remote bookmark master_bookmark from * to * (glob)
