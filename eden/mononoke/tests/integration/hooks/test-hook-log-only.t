# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup \
  > block_commit_message_pattern <(
  >   cat <<CONF
  > log_only=true
  > config_json='''{
  >   "pattern": "([@]nocommit)",
  >   "message": "Message contains nocommit marker"
  > }'''
  > CONF
  > )

  $ hg up -q tip

Push a commit that fails the hook, it is still allowed as the hook is log-only.

  $ echo "foo" >> foo
  $ hg ci -Aqm $"Contains @""nocommit"

  $ hg push -r . --to master_bookmark
  pushing rev d379d7937ea5 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

  $ jq < $TESTTMP/hooks-scuba.json -c '[.normal.hook, .int.failed_hooks, .normal.log_only_rejection]'
  ["block_commit_message_pattern",0,"Message contains nocommit marker"]
