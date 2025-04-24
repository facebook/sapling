# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup \
  >  block_content_pattern \
  >    <(echo 'config_json="{\"pattern\":\"([@]nocommit)\",\"message\":\"File contains ${1}\"}"') \
  >  block_commit_message_pattern \
  >    <(echo 'config_json="{\"pattern\":\"([@]nocommit)\",\"message\":\"Message contains ${1}\"}"')

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

No marker, should work

  $ echo "foo" > foo
  $ hg ci -Aqm 1
  $ hg push -r . --to master_bookmark
  pushing rev b27569b9b813 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Has marker in the title

  $ echo "foo" >> foo
  $ hg ci -Aqm $"My imperfect commit\nI've used @""nocommit so it's never commited"
  $ hg push -r . --to master_bookmark
  pushing rev c47f23c6877c to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_commit_message_pattern for c47f23c6877c12dc04d6123604338f60359ad73b: Message contains [@]nocommit (re)
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hg hide -q .

Has marker in a file, should fail

  $ echo "bar @""nocommit" > foo
  $ hg ci -Aqm 1
  $ hg push -r . --to master_bookmark
  pushing rev 439123601c36 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_content_pattern for 439123601c36d157e6d3be53a1bbce33592ccdf3: File contains [@]nocommit: foo (re)
  abort: unexpected EOL, expected netstring digit
  [255]
