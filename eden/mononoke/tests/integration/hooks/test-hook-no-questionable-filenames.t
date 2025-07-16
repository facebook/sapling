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
  $ hg push -r . --to master_bookmark
  pushing rev 6e530c466555 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 6e530c466555faea64db6bd6425dbfe684b65afc: ABORT: Illegal filename: test/foo bar. The file name cannot include spaces, apostrophes or start with hyphens.
  abort: unexpected EOL, expected netstring digit
  [255]

Attempt to add a filename with braces in it
  $ hg up -q "min(all())"
  $ mkdir -p "test"
  $ echo "bad" > "test/{foobar}"
  $ hg ci -Aqm success
  $ hg push -r . --to master_bookmark
  pushing rev 089829221aab to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 089829221aab7ce82a51c996ce41806affbfb765: ABORT: Illegal filename: "test/{foobar}". The file name cannot include brace(s).
  abort: unexpected EOL, expected netstring digit
  [255]

Attempt to add a filename with a hypen at the start
  $ hg up -q "min(all())"
  $ echo "good" > -testfile
  $ hg ci -Aqm good
  $ hg push -r . --to master_bookmark
  pushing rev 63e8cc599c6f to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for 63e8cc599c6f7c9e52c088da857f9cb4b9499160: ABORT: Illegal filename: -testfile. The file name cannot include spaces, apostrophes or start with hyphens.
  abort: unexpected EOL, expected netstring digit
  [255]

Attempt to add a filename with an apostrophe in it
  $ hg up -q "min(all())"
  $ echo "bad" > "test'file"
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev c82854fd39eb to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_questionable_filenames for c82854fd39ebf2f9811187c8c32756d811dd2907: ABORT: Illegal filename: test'file. The file name cannot include spaces, apostrophes or start with hyphens.
  abort: unexpected EOL, expected netstring digit
  [255]
