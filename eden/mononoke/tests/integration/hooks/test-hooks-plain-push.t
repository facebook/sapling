# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
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
  $ hg pull -q -B master_bookmark -B alternate

make more commits
  $ A_HASH=$(hg log -r 'remote/master_bookmark' -T '{node}')
  $ X_HASH=$(hg log -r 'remote/alternate' -T '{node}')
  $ drawdag <<EOF
  > D F           # C/large = file_too_large
  > | |           # E/large = file_too_large
  > C E    Z      # Y/large = file_too_large
  > |/     |
  > B      Y
  > |      |
  > |      $X_HASH
  > $A_HASH
  > EOF

fast-forward the bookmark
  $ hg up -q $B
  $ hg push -q -r . --to master_bookmark

fast-forward the bookmark over a commit that fails the hook
  $ hg up -q $D
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for *: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes, 1.40x the limit). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions. (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

bypass the hook, the push will now work
  $ hg push -q -r . --to master_bookmark --pushvar ALLOW_LARGE_FILES=true

attempt a non-fast-forward move, it should fail
  $ hg up -q $F
  $ hg push -r . --to master_bookmark --non-forward-move
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:     Caused by:
  remote:         0: Failed to fast-forward bookmark (set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)
  remote:         1: Non fast-forward bookmark move of 'master_bookmark' from * to * (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

allow the non-forward move
  $ hg push -r . --to master_bookmark --non-forward-move --pushvar NON_FAST_FORWARD=true
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  no changes found (?)
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for *: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes, 1.40x the limit). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions. (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

bypass the hook too, and it should work
  $ hg push -q -r . --to master_bookmark --non-forward-move --pushvar NON_FAST_FORWARD=true --pushvar ALLOW_LARGE_FILES=true

attempt a move to a completely unrelated commit (no common ancestor), with an ancestor that
fails the hook
  $ hg up -q $Z
  $ hg push -r . --to master_bookmark --non-forward-move --pushvar NON_FAST_FORWARD=true
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  no changes found (?)
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for *: File size limit is 10 bytes. You tried to push file large that is over the limit (14 bytes, 1.40x the limit). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions. (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

bypass the hook, and it should work
  $ hg push -q -r . --to master_bookmark --non-forward-move --pushvar NON_FAST_FORWARD=true --pushvar ALLOW_LARGE_FILES=true
