# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ export LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 LANGUAGE=en_US.UTF-8

  $ hook_test_setup \
  > limit_path_length <(
  >   cat <<CONF
  > config_json='''{
  >   "length_limit": 10
  > }'''
  > CONF
  > )

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Ok file path - should work

  $ touch 123456789
  $ hg ci -Aqm 1
  $ hg push -r . --to master_bookmark
  pushing rev f6dd4142eb31 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

File path too long - should fail

  $ hg up -q "min(all())"
  $ touch 1234567890
  $ hg ci -Aqm 1
  $ hg push -r . --to master_bookmark
  pushing rev 9318eba87175 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_path_length for 9318eba871756ffacc9f241d4b673747b1f41126: Path length for '1234567890' (10) exceeds length limit (>= 10)
  abort: unexpected EOL, expected netstring digit
  [255]

File path too long (UTF-8 multibyte characters) - should fail

  $ hg up -q "min(all())"
  $ touch 12345678€
  $ hg ci -Aqm 1
  $ hg push -r . --to master_bookmark
  pushing rev a75c9951ef38 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_path_length for a75c9951ef38ca961a461967925b919bb00e11f0: Path length for '12345678\xe2\x82\xac' (11) exceeds length limit (>= 10) (esc)
  abort: unexpected EOL, expected netstring digit
  [255]
