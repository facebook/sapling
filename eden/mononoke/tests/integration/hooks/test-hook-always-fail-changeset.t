# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ export LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 LANGUAGE=en_US.UTF-8
  $ export HOOKBOOKMARK_REGEX="(master)|(release_v[0-9]+)"


# Test that it's backwards compatible and runs without any configs
  $ hook_test_setup always_fail_changeset

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved


Doesn no match regex - should pass
  $ mkcommit 1
  $ hg push -r . --to random_bookmark --create
  pushing rev c2e526aacb51 to destination mono:repo bookmark random_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark random_bookmark


Matches regex - should NOT pass

  $ hg up -q "min(all())"
  $ mkcommit 2
  $ hg push -r . --to master --create
  pushing rev c9b2673d3218 to destination mono:repo bookmark master
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     always_fail_changeset for *: This hook always fails (glob)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     always_fail_changeset for *: This hook always fails (glob)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nalways_fail_changeset for *: This hook always fails" (glob)
  abort: unexpected EOL, expected netstring digit
  [255]
  $ mkcommit another
  $ hg push -r . --to release_v1 --create
  pushing rev cddcb6a5bb53 to destination mono:repo bookmark release_v1
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     always_fail_changeset for *: This hook always fails (glob)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     always_fail_changeset for *: This hook always fails (glob)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nalways_fail_changeset for *: This hook always fails" (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

# Add custom rejection message
  $ hook_test_setup always_fail_changeset <(
  >   cat <<CONF
  > config_json='''{
  >   "message": "This bookmark is locked and does not accept pushes"
  > }'''
  > CONF
  > )
  abort: repository `$TESTTMP/repo` already exists
  abort: destination 'repo2' is not empty
  $ force_update_configerator
  $ hg push -r . --to master --create
  pushing rev cddcb6a5bb53 to destination mono:repo bookmark master
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     always_fail_changeset for *: This bookmark is locked and does not accept pushes (glob)
  remote:     always_fail_changeset for *: This bookmark is locked and does not accept pushes (glob)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     always_fail_changeset for *: This bookmark is locked and does not accept pushes (glob)
  remote:     always_fail_changeset for *: This bookmark is locked and does not accept pushes (glob)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nalways_fail_changeset for *: This bookmark is locked and does not accept pushes\nalways_fail_changeset for *: This bookmark is locked and does not accept pushes" (glob)
  abort: unexpected EOL, expected netstring digit
  [255]
