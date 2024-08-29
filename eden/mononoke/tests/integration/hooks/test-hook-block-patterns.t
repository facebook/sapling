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
  pushing rev 8b8214d70c17 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Has marker in the title

  $ echo "foo" >> foo
  $ hg ci -Aqm $"My imperfect commit\nI've used @""nocommit so it's never commited"
  $ hg push -r . --to master_bookmark
  pushing rev 228cf1cc53cb to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_commit_message_pattern for 228cf1cc53cb54cea0499f899fe8063b07b42e01: Message contains [@]nocommit (re)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_commit_message_pattern for 228cf1cc53cb54cea0499f899fe8063b07b42e01: Message contains [@]nocommit (re)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\\nblock_commit_message_pattern for 228cf1cc53cb54cea0499f899fe8063b07b42e01: Message contains [@]nocommit" (re)
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hg hide -q .

Has marker in a file, should fail

  $ echo "bar @""nocommit" > foo
  $ hg ci -Aqm 1
  $ hg push -r . --to master_bookmark
  pushing rev b950c81d785b to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_content_pattern for b950c81d785b1d845bd12055189cb3b63c9c8a1b: File contains [@]nocommit: foo (re)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_content_pattern for b950c81d785b1d845bd12055189cb3b63c9c8a1b: File contains [@]nocommit: foo (re)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\\nblock_content_pattern for b950c81d785b1d845bd12055189cb3b63c9c8a1b: File contains [@]nocommit: foo" (re)
  abort: unexpected EOL, expected netstring digit
  [255]
