# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"


  $ hook_test_setup \
  > limit_commit_message_length <(
  >   cat <<CONF
  > config_json='''{
  >   "length_limit": 10,
  >   "display_title_length": 5
  > }'''
  > CONF
  > )

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Ok commit message - should pass

  $ touch file1
  $ hg ci -Aqm 123456789
  $ hgmn push -r . --to master_bookmark
  pushing rev f95217ebe3a8 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Commit message too long - should fail

  $ hg up -q "min(all())"
  $ touch file2
  $ hg ci -Aqm "$(printf "%s\n%s" "foo" "123456")"
  $ hgmn push -r . --to master_bookmark
  pushing rev 6ef9fe6a13fa to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_message_length for 6ef9fe6a13fa92ed3a2fdc0843441c0511cd47f6: Commit message length for 'foo' (10) exceeds length limit (>= 10)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_commit_message_length for 6ef9fe6a13fa92ed3a2fdc0843441c0511cd47f6: Commit message length for 'foo' (10) exceeds length limit (>= 10)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_commit_message_length for 6ef9fe6a13fa92ed3a2fdc0843441c0511cd47f6: Commit message length for 'foo' (10) exceeds length limit (>= 10)"
  abort: unexpected EOL, expected netstring digit
  [255]

Commit message too long (UTF-8 multibyte characters) - should fail

  $ hg up -q "min(all())"
  $ touch file3
  $ hg ci -Aqm "$(printf "%s\n%s" "title_SHOULD BE STRIPPED IN MSG" "1234€")"
  $ hgmn push -r . --to master_bookmark
  pushing rev e363a5d26663 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_message_length for e363a5d266639d657f163422864c1e0259f88760: Commit message length for 'title' (39) exceeds length limit (>= 10)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_commit_message_length for e363a5d266639d657f163422864c1e0259f88760: Commit message length for 'title' (39) exceeds length limit (>= 10)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_commit_message_length for e363a5d266639d657f163422864c1e0259f88760: Commit message length for 'title' (39) exceeds length limit (>= 10)"
  abort: unexpected EOL, expected netstring digit
  [255]
