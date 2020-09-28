# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ export LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 LANGUAGE=en_US.UTF-8

  $ hook_test_setup \
  > limit_path_length <(
  >   cat <<CONF
  > config_strings={length_limit="10"}
  > CONF
  > )

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Ok file path - should work

  $ touch 123456789
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 2f6ac546dc81 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

File path too long - should fail

  $ hg up -q 0
  $ touch 1234567890
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 56fa24a52883 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_path_length for 56fa24a5288379b752543077df52a8da6d6113ec: Path length for '1234567890' (10) exceeds length limit (>= 10)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_path_length for 56fa24a5288379b752543077df52a8da6d6113ec: Path length for '1234567890' (10) exceeds length limit (>= 10)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_path_length for 56fa24a5288379b752543077df52a8da6d6113ec: Path length for \'1234567890\' (10) exceeds length limit (>= 10)"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

File path too long (UTF-8 multibyte characters) - should fail

  $ hg up -q 0
  $ touch 12345678€
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 2aa9727c0ca2 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_path_length for 2aa9727c0ca277205aedda2a1acf9d077eafc9d5: Path length for '12345678\xe2\x82\xac' (11) exceeds length limit (>= 10) (esc)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_path_length for 2aa9727c0ca277205aedda2a1acf9d077eafc9d5: Path length for '12345678\xe2\x82\xac' (11) exceeds length limit (>= 10) (esc)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\\nlimit_path_length for 2aa9727c0ca277205aedda2a1acf9d077eafc9d5: Path length for \\'12345678\xe2\x82\xac\\' (11) exceeds length limit (>= 10)" (esc)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
