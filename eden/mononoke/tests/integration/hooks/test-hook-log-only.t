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
  $ echo "bar" >> bar 
  $ hg ci -Aqm $"This one is not blocked and should still be logged"
  $ echo "baz" >> baz
  $ hg ci -Aqm $"This one should also be logged, but not bypassed"

  $ hg push -r . --to master_bookmark
  pushing rev 9dd17be63f49 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

  $ jq < $TESTTMP/hooks-scuba.json -c '[.normal.hook, .int.failed_hooks, .normal.hash, .normal.log_only_rejection]' | sort
  ["block_commit_message_pattern",0,"0a36ab9f8e48edffb56ff84ff629c77cb5db82cb33a538c85d45e744d40a170e",null]
  ["block_commit_message_pattern",0,"dc28fde1d0208243d3f8e973e9be4538bb366bfae7e210d95fcb40b4acea5a65","Message contains nocommit marker"]
  ["block_commit_message_pattern",0,"dd74b203799ebbd374a139e4ad2b25def6f6f5b3bade5d4c1030b66d146f666b",null]
