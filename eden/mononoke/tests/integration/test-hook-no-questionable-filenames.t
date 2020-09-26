# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup no_questionable_filenames <( \
  >   echo 'bypass_pushvar="ALLOW_CRAZY_FILENAMES=true"'
  > )

Attempt to add a filename with spaces in it
  $ hg up -q 0
  $ mkdir -p "test"
  $ echo "bad" > "test/foo bar"
  $ hg ci -Aqm success
  $ hgmn push -r . --to master_bookmark
  pushing rev c60235ea2c7f to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for c60235ea2c7ff0fbb5fd0e1e9906fb712b7853d0: ABORT: Illegal filename: test/foo bar
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_questionable_filenames for c60235ea2c7ff0fbb5fd0e1e9906fb712b7853d0: ABORT: Illegal filename: test/foo bar
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_questionable_filenames for c60235ea2c7ff0fbb5fd0e1e9906fb712b7853d0: ABORT: Illegal filename: test/foo bar"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Attempt to add a filename with braces in it
  $ hg up -q 0
  $ mkdir -p "test"
  $ echo "bad" > "test/{foobar}"
  $ hg ci -Aqm success
  $ hgmn push -r . --to master_bookmark
  pushing rev 8d7d42b0b3af to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 8d7d42b0b3afdb18551c0e69751d044c68e1906b: ABORT: Illegal filename: test/{foobar}
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 8d7d42b0b3afdb18551c0e69751d044c68e1906b: ABORT: Illegal filename: test/{foobar}
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_questionable_filenames for 8d7d42b0b3afdb18551c0e69751d044c68e1906b: ABORT: Illegal filename: test/{foobar}"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Attempt to add a filename with a hypen at the start
  $ hg up -q 0
  $ echo "good" > -testfile
  $ hg ci -Aqm good
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for b2b56d66a7073312c059555f1193c5183cf8d37f: ABORT: Illegal filename: -testfile
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_questionable_filenames for b2b56d66a7073312c059555f1193c5183cf8d37f: ABORT: Illegal filename: -testfile
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_questionable_filenames for b2b56d66a7073312c059555f1193c5183cf8d37f: ABORT: Illegal filename: -testfile"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Attempt to add a filename with an apostrophe in it
  $ hg up -q 0
  $ echo "bad" > "test'file"
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 11ee725a331757675c477522b172ab35967903ef: ABORT: Illegal filename: test'file
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 11ee725a331757675c477522b172ab35967903ef: ABORT: Illegal filename: test'file
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_questionable_filenames for 11ee725a331757675c477522b172ab35967903ef: ABORT: Illegal filename: test\'file"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
