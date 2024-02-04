# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# This tests @nocommit, so we need to suppress the lint
# @lint-ignore-every NOCOMMIT

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup check_nocommit "" check_nocommit_message ""

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

No @nocommit, should work

  $ echo "foo" > foo
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 8b8214d70c17 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Has @nocommit in the title

  $ echo "foo" >> foo
  $ hg ci -Aqm $"My imperfect commit\nI've used @nocommit so it's never commited"
  $ hgmn push -r . --to master_bookmark
  pushing rev 228cf1cc53cb to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     check_nocommit_message for 228cf1cc53cb54cea0499f899fe8063b07b42e01: Commit message contains a nocommit marker: @nocommit
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     check_nocommit_message for 228cf1cc53cb54cea0499f899fe8063b07b42e01: Commit message contains a nocommit marker: @nocommit
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\ncheck_nocommit_message for 228cf1cc53cb54cea0499f899fe8063b07b42e01: Commit message contains a nocommit marker: @nocommit"
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hg hide -q .

Has @nocommit, should fail

  $ echo "bar @nocommit" > foo
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev b950c81d785b to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     check_nocommit for b950c81d785b1d845bd12055189cb3b63c9c8a1b: File contains a @nocommit marker: foo
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     check_nocommit for b950c81d785b1d845bd12055189cb3b63c9c8a1b: File contains a @nocommit marker: foo
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\ncheck_nocommit for b950c81d785b1d845bd12055189cb3b63c9c8a1b: File contains a @nocommit marker: foo"
  abort: unexpected EOL, expected netstring digit
  [255]
