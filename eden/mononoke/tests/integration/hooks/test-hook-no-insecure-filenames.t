# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ export LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 LANGUAGE=en_US.UTF-8

  $ hook_test_setup no_insecure_filenames <( \
  >   echo 'bypass_pushvar="TEST_ONLY_ALLOW_INSECURE_FILENAMES=true"'
  > )

Add a .hg(sub|tags|substate) file
  $ hg up -q "min(all())"
  $ echo "bad" > .hgtags
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev 448afa68ffb8 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 448afa68ffb821f384ffe7a3691eebfc07a9b7dc: ABORT: Illegal filename: ".hgtags"
  abort: unexpected EOL, expected netstring digit
  [255]

Add a legitimate file with hg in its name
  $ hg up -q "min(all())"
  $ echo "good" > .hgsubstatefoo
  $ hg ci -Aqm good
  $ hg push -r . --to master_bookmark
  pushing rev e7a8c022dfae to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Add a dir with a naughty .Git directory inside
  $ hg up -q "min(all())"
  $ mkdir -p test/.Git/
  $ echo "bad" > test/.Git/test.py
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev ebe6271ae0b5 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for ebe6271ae0b537ca3f18d4cd7c24cc27fca67c6d: ABORT: Illegal insecure name: "test/.Git/test.py"
  abort: unexpected EOL, expected netstring digit
  [255]

Add a dir with a naughty .git directory inside
  $ hg up -q "min(all())"
  $ mkdir -p test/.git/
  $ echo "bad" > test/.git/test.py
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev aaf08d8ead79 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for aaf08d8ead79addcb96d5db66cb7d507994e378f: ABORT: Illegal insecure name: "test/.git/test.py"
  abort: unexpected EOL, expected netstring digit
  [255]

Add a dir with a naughty .git directory inside that includes a ~1
  $ hg up -q "min(all())"
  $ mkdir -p test/.Git~1/
  $ echo "bad" > test/.Git~1/test.py
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev b96385640c60 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for b96385640c60aef792315cde058ff8f935f26b91: ABORT: Illegal insecure name: "test/.Git~1/test.py"
  abort: unexpected EOL, expected netstring digit
  [255]

Add a dir with a naughty .git directory inside that includes a ~1234
  $ hg up -q "min(all())"
  $ mkdir -p test/.Git~1234/test
  $ echo "bad" > test/.Git~1234/test/test.py
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev b3272e728d01 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for b3272e728d0198a15cbd84f4028fd92f1aa518b0: ABORT: Illegal insecure name: "test/.Git~1234/test/test.py"
  abort: unexpected EOL, expected netstring digit
  [255]

Add a bad dir
  $ hg up -q "min(all())"
  $ mkdir -p dir1/.Git8B6C~2
  $ echo "bad" > dir1/.Git8B6C~2/file1
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev 1042a7c7a32b to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 1042a7c7a32bdca062a757a3bec4b2b1733030cb: ABORT: Illegal insecure name: "dir1/.Git8B6C~2/file1"
  abort: unexpected EOL, expected netstring digit
  [255]

Add a dir with a naughty .git directory inside that includes 2 ~1
  $ hg up -q "min(all())"
  $ mkdir -p test~1/.Git~1/test
  $ echo "bad" > test~1/.Git~1/test/test.py
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev 9d9c01a3e22f to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 9d9c01a3e22feb46d7ab85107213b6415807ea4d: ABORT: Illegal insecure name: "test~1/.Git~1/test/test.py"
  abort: unexpected EOL, expected netstring digit
  [255]

Add a legitimate dir with git in its name
  $ hg up -q "min(all())"
  $ mkdir -p test/git/
  $ echo "good" > test/git/test.py
  $ hg ci -Aqm good
  $ hg push -r . --to master_bookmark
  pushing rev 9c9e2f225bd8 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Add a legitimate dir with jgit in its name
  $ hg up -q "min(all())"
  $ echo "good" > jgit
  $ hg ci -Aqm good
  $ hg push -r . --to master_bookmark
  pushing rev 6aa1c965bdf6 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Add a legitimate dir with xGit in its name
  $ hg up -q "min(all())"
  $ mkdir -p test/xGit/
  $ echo "good" > test/xGit/test.py
  $ hg ci -Aqm good
  $ hg push -r . --to master_bookmark
  pushing rev 6fcb3bd41475 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Add a file with an ignorable unicode char in it
  $ hg up -q "min(all())"
  $ bad=$(printf "\xe2\x80\x8c")
  $ mkdir test
  $ echo "bad" > "test/.git${bad}"
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev 3396def5223f to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_insecure_filenames for 3396def5223f7db8697ed4b93114036d977c6c5c: ABORT: Illegal insecure name: "test/.git\u{200c}"
  abort: unexpected EOL, expected netstring digit
  [255]


Check that we can delete insecure filenames
--add a normally prohibited filename with a pushvar
  $ hg up -q "min(all())"
  $ echo "bad" > .hgtags
  $ hg ci -Aqm insequre_filename
  $ hg push -qr . --to master_bookmark --pushvars TEST_ONLY_ALLOW_INSECURE_FILENAMES=true

-- delete just-added insecure filename
  $ hg up -q master_bookmark
  $ hg rm .hgtags
  $ hg ci -qm "remove tags"
  $ hg push -qr . --to master_bookmark
