# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ hook_test_setup \
  > block_noconflict_merge_commits

  $ hg up -q tip
  $ echo left > left && hg -q addremove && hg commit -m "left"
  $ hg prev -q
  [0a489c] c
  $ echo right > right && hg -q addremove && hg commit -m "right"

A merge created with --noconflict is rejected, even when the client-side push
guard is switched off
  $ hg merge --noconflict -m clean 'desc(left)'
  created conflict-free merge * (glob)
  $ hg push -r . --to master_bookmark --config push.reject-noconflict-merges=false
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_noconflict_merge_commits for *: This merge was created with 'sl merge --noconflict'. It only records that its parents can be merged automatically and must not be landed; land its parents and descendants instead. (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

A plain merge of the same commits is not this hook's concern
  $ hg up -q 'desc(right)'
  $ hg merge -q 'desc(left)'
  $ hg commit -m "plain merge"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
