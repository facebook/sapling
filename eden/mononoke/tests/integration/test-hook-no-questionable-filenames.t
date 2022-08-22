# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup no_questionable_filenames <( \
  >   echo 'bypass_pushvar="ALLOW_CRAZY_FILENAMES=true"'
  > )

Attempt to add a filename with spaces in it
  $ hg up -q "min(all())"
  $ mkdir -p "test"
  $ echo "bad" > "test/foo bar"
  $ hg ci -Aqm success
  $ hgmn push -r . --to master_bookmark
  pushing rev c60235ea2c7f to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for c60235ea2c7ff0fbb5fd0e1e9906fb712b7853d0: ABORT: Illegal filename: test/foo bar. The file name cannot include spaces, apostrophes or start with hyphens.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_questionable_filenames for c60235ea2c7ff0fbb5fd0e1e9906fb712b7853d0: ABORT: Illegal filename: test/foo bar. The file name cannot include spaces, apostrophes or start with hyphens.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_questionable_filenames for c60235ea2c7ff0fbb5fd0e1e9906fb712b7853d0: ABORT: Illegal filename: test/foo bar. The file name cannot include spaces, apostrophes or start with hyphens."
  abort: unexpected EOL, expected netstring digit
  [255]

Attempt to add a filename with braces in it
  $ hg up -q "min(all())"
  $ mkdir -p "test"
  $ echo "bad" > "test/{foobar}"
  $ hg ci -Aqm success
  $ hgmn push -r . --to master_bookmark
  pushing rev 8d7d42b0b3af to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 8d7d42b0b3afdb18551c0e69751d044c68e1906b: ABORT: Illegal filename: test/{foobar}. The file name cannot include brace(s).
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 8d7d42b0b3afdb18551c0e69751d044c68e1906b: ABORT: Illegal filename: test/{foobar}. The file name cannot include brace(s).
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_questionable_filenames for 8d7d42b0b3afdb18551c0e69751d044c68e1906b: ABORT: Illegal filename: test/{foobar}. The file name cannot include brace(s)."
  abort: unexpected EOL, expected netstring digit
  [255]

Attempt to add a filename with a hypen at the start
  $ hg up -q "min(all())"
  $ echo "good" > -testfile
  $ hg ci -Aqm good
  $ hgmn push -r . --to master_bookmark
  pushing rev b2b56d66a707 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for b2b56d66a7073312c059555f1193c5183cf8d37f: ABORT: Illegal filename: -testfile. The file name cannot include spaces, apostrophes or start with hyphens.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_questionable_filenames for b2b56d66a7073312c059555f1193c5183cf8d37f: ABORT: Illegal filename: -testfile. The file name cannot include spaces, apostrophes or start with hyphens.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_questionable_filenames for b2b56d66a7073312c059555f1193c5183cf8d37f: ABORT: Illegal filename: -testfile. The file name cannot include spaces, apostrophes or start with hyphens."
  abort: unexpected EOL, expected netstring digit
  [255]

Attempt to add a filename with an apostrophe in it
  $ hg up -q "min(all())"
  $ echo "bad" > "test'file"
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev 11ee725a3317 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 11ee725a331757675c477522b172ab35967903ef: ABORT: Illegal filename: test'file. The file name cannot include spaces, apostrophes or start with hyphens.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 11ee725a331757675c477522b172ab35967903ef: ABORT: Illegal filename: test'file. The file name cannot include spaces, apostrophes or start with hyphens.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_questionable_filenames for 11ee725a331757675c477522b172ab35967903ef: ABORT: Illegal filename: test'file. The file name cannot include spaces, apostrophes or start with hyphens."
  abort: unexpected EOL, expected netstring digit
  [255]
