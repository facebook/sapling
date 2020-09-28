# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ export LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 LANGUAGE=en_US.UTF-8

  $ hook_test_setup no_insecure_filenames

Add a .hg(sub|tags|substate) file
  $ hg up -q 0
  $ echo "bad" > .hgtags
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 42be02defdeedc5825555cc9adbbf537b1bf1c49: ABORT: Illegal filename: .hgtags
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 42be02defdeedc5825555cc9adbbf537b1bf1c49: ABORT: Illegal filename: .hgtags
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_insecure_filenames for 42be02defdeedc5825555cc9adbbf537b1bf1c49: ABORT: Illegal filename: .hgtags"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Add a legitimate file with hg in its name
  $ hg up -q 0
  $ echo "good" > .hgsubstatefoo
  $ hg ci -Aqm good
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Add a dir with a naughty .Git directory inside
  $ hg up -q 0
  $ mkdir -p test/.Git/
  $ echo "bad" > test/.Git/test.py
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 63a821ce8ce6d0e38385bb41f49a77b46d1d81a1: ABORT: Illegal insecure name: test/.Git/test.py
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 63a821ce8ce6d0e38385bb41f49a77b46d1d81a1: ABORT: Illegal insecure name: test/.Git/test.py
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_insecure_filenames for 63a821ce8ce6d0e38385bb41f49a77b46d1d81a1: ABORT: Illegal insecure name: test/.Git/test.py"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Add a dir with a naughty .git directory inside
  $ hg up -q 0
  $ mkdir -p test/.git/
  $ echo "bad" > test/.git/test.py
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 214bf1e67d4847fabd9a134bae0a1bf466fea704: ABORT: Illegal insecure name: test/.git/test.py
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 214bf1e67d4847fabd9a134bae0a1bf466fea704: ABORT: Illegal insecure name: test/.git/test.py
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_insecure_filenames for 214bf1e67d4847fabd9a134bae0a1bf466fea704: ABORT: Illegal insecure name: test/.git/test.py"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Add a dir with a naughty .git directory inside that includes a ~1
  $ hg up -q 0
  $ mkdir -p test/.Git~1/
  $ echo "bad" > test/.Git~1/test.py
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 7800fe789a874b225e4974fa09a25a051ea3d1e0: ABORT: Illegal insecure name: test/.Git~1/test.py
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 7800fe789a874b225e4974fa09a25a051ea3d1e0: ABORT: Illegal insecure name: test/.Git~1/test.py
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_insecure_filenames for 7800fe789a874b225e4974fa09a25a051ea3d1e0: ABORT: Illegal insecure name: test/.Git~1/test.py"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Add a dir with a naughty .git directory inside that includes a ~1234
  $ hg up -q 0
  $ mkdir -p test/.Git~1234/test
  $ echo "bad" > test/.Git~1234/test/test.py
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 8e508312f2d6a7f354ee17bc46a9dc618da9ded3: ABORT: Illegal insecure name: test/.Git~1234/test/test.py
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 8e508312f2d6a7f354ee17bc46a9dc618da9ded3: ABORT: Illegal insecure name: test/.Git~1234/test/test.py
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_insecure_filenames for 8e508312f2d6a7f354ee17bc46a9dc618da9ded3: ABORT: Illegal insecure name: test/.Git~1234/test/test.py"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Add a bad dir
  $ hg up -q 0
  $ mkdir -p dir1/.Git8B6C~2
  $ echo "bad" > dir1/.Git8B6C~2/file1
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 695a2a5c3e7ce0fdccefa1945c8bd8868027248b: ABORT: Illegal insecure name: dir1/.Git8B6C~2/file1
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 695a2a5c3e7ce0fdccefa1945c8bd8868027248b: ABORT: Illegal insecure name: dir1/.Git8B6C~2/file1
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_insecure_filenames for 695a2a5c3e7ce0fdccefa1945c8bd8868027248b: ABORT: Illegal insecure name: dir1/.Git8B6C~2/file1"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Add a dir with a naughty .git directory inside that includes 2 ~1
  $ hg up -q 0
  $ mkdir -p test~1/.Git~1/test
  $ echo "bad" > test~1/.Git~1/test/test.py
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 014b76ac58ed568649b5308bece3aa75aefceca8: ABORT: Illegal insecure name: test~1/.Git~1/test/test.py
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 014b76ac58ed568649b5308bece3aa75aefceca8: ABORT: Illegal insecure name: test~1/.Git~1/test/test.py
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_insecure_filenames for 014b76ac58ed568649b5308bece3aa75aefceca8: ABORT: Illegal insecure name: test~1/.Git~1/test/test.py"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Add a legitimate dir with git in its name
  $ hg up -q 0
  $ mkdir -p test/git/
  $ echo "good" > test/git/test.py
  $ hg ci -Aqm good
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Add a legitimate dir with jgit in its name
  $ hg up -q 0
  $ echo "good" > jgit
  $ hg ci -Aqm good
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Add a legitimate dir with xGit in its name
  $ hg up -q 0
  $ mkdir -p test/xGit/
  $ echo "good" > test/xGit/test.py
  $ hg ci -Aqm good
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Add a file with an ignorable unicode char in it
  $ hg up -q 0
  $ bad=$(printf "\xe2\x80\x8c")
  $ mkdir test
  $ echo "bad" > "test/.git${bad}"
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 673dc62e3d09668ca2ef53b04d2527dd3c8e0b2e: ABORT: Illegal insecure name: test/.git\xe2\x80\x8c (esc)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 673dc62e3d09668ca2ef53b04d2527dd3c8e0b2e: ABORT: Illegal insecure name: test/.git\xe2\x80\x8c (esc)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_insecure_filenames for 673dc62e3d09668ca2ef53b04d2527dd3c8e0b2e: ABORT: Illegal insecure name: test/.git\u{200c}"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
