# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ hook_test_setup \
  > block_empty_commit

  $ hg up -q tip

  $ echo 1 > 1 && hg -q addremove && hg ci -m empty
  $ hg revert -r ".^" 1 && hg commit --amend
  $ hg push -r . --to master_bookmark
  pushing rev 86565fe0334c to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_empty_commit for 86565fe0334c5e08c8ebc2fd57d96d9ed0986e7d: You must include file changes in your commit for it to land
  abort: unexpected EOL, expected netstring digit
  [255]

  $ echo 1 > 1 && hg addremove && hg ci --amend -m nonempty
  adding 1
  $ hg push -r . --to master_bookmark
  pushing rev 4d6d9aeac877 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
