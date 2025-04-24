# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup \
  > block_files <(
  >   cat <<CONF
  > bypass_pushvar="TEST_BYPASS=true"
  > bypass_commit_string="@bypass_block_files"
  > config_json='''{
  >   "block_patterns": [
  >     "^buck-out/",
  >     "/buck-out/",
  >     "DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED",
  >     "^owners$",
  >     "^OWNERS$",
  >     ".*/owners$",
  >     ".*/OWNERS$",
  >     "/\\\\.git/",
  >     "^\\\\.git/",
  >     ".*/\\\\.watchmanconfig$",
  >     "^arvr-legacy/",
  >     "^\\\\.ovrsource-rest/",
  >     "^\\\\.fbsource-rest/",
  >     "^fbcode/_bin/",
  >     "^fbandroid/fbandroid/",
  >     "^fbcode/fbcode/",
  >     "^fbobjc/fbobjc/",
  >     "^xplat/xplat/",
  >     "^xplat/fbandroid/",
  >     "^xplat/fbcode/",
  >     "^xplat/fbobjc/",
  >     "^fbcode/experimental",
  >     "^fbcode/tupperware/config/experimental",
  >     "^fbandroid/experimental",
  >     "^fbobjc/Users",
  >     "^xplat/experimental"
  >   ]
  > }'''
  > CONF
  > )

Negative testing
  $ hg up -q "min(all())"
  $ echo "good" > good_file.txt
  $ hg ci -Aqm negative
  $ hg push -r . --to master_bookmark
  pushing rev 02f7f604f5e4 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Tricky case - this should succeed, but looks very similar to cases that should not
  $ hg up -q "min(all())"
  $ mkdir -p test-buck-out/buck-out-test/
  $ echo "good" > test-buck-out/buck-out-test/buck-out
  $ hg ci -Aqm negative
  $ hg push -r . --to master_bookmark
  pushing rev 59a1697a67a1 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

buck-out directory is not allowed in the root
  $ hg up -q "min(all())"
  $ mkdir -p buck-out/
  $ echo "bad" > buck-out/file
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev 2e29df6a828d to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 2e29df6a828daec5228e2142b806345778776871: Blocked filename 'buck-out/file' matched name pattern '^buck-out/'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

buck-out directory is not allowed in any subdir
  $ hg up -q "min(all())"
  $ mkdir -p dir/buck-out
  $ echo "bad" > dir/buck-out/file
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev c2eff965c3d8 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for c2eff965c3d873332c55d7dc2d29a73e91f31ffd: Blocked filename 'dir/buck-out/file' matched name pattern '/buck-out/'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED does the needful
  $ hg up -q "min(all())"
  $ echo "bad" > important_DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED.txt
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev 21ab636d0b9f to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 21ab636d0b9f382939cb3740b6a93909729142b7: Blocked filename 'important_DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED.txt' matched name pattern 'DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

Old fbmake leftovers cannot be committed
  $ hg up -q "min(all())"
  $ mkdir -p fbcode/_bin
  $ echo "bad" > fbcode/_bin/file
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev 1c764df4be5b to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 1c764df4be5b9a7f42ba5f9f5e2e64faea60f180: Blocked filename 'fbcode/_bin/file' matched name pattern '^fbcode/_bin/'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

Cannot nest project dirs badly
  $ hg up -q "min(all())"
  $ for path in fbandroid/fbandroid fbcode/fbcode fbobjc/fbobjc xplat/xplat; do
  > mkdir -p $path
  > echo fail > $path/files
  > done
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark |& grep -P 'remote:\s+block_files for' | sort
  remote:     block_files for 1e6669d90ee6a9825c530a8332bcb0edd3dfd4a3: Blocked filename 'fbandroid/fbandroid/files' matched name pattern '^fbandroid/fbandroid/'. Rename or remove this file and try again.
  remote:     block_files for 1e6669d90ee6a9825c530a8332bcb0edd3dfd4a3: Blocked filename 'fbcode/fbcode/files' matched name pattern '^fbcode/fbcode/'. Rename or remove this file and try again.
  remote:     block_files for 1e6669d90ee6a9825c530a8332bcb0edd3dfd4a3: Blocked filename 'fbobjc/fbobjc/files' matched name pattern '^fbobjc/fbobjc/'. Rename or remove this file and try again.
  remote:     block_files for 1e6669d90ee6a9825c530a8332bcb0edd3dfd4a3: Blocked filename 'xplat/xplat/files' matched name pattern '^xplat/xplat/'. Rename or remove this file and try again.

Cannot put crud in xplat
  $ hg up -q "min(all())"
  $ for path in xplat/fbandroid xplat/fbcode xplat/fbobjc; do
  > mkdir -p $path
  > echo fail > $path/files
  > done
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark |& grep -P 'remote:\s+block_files for' | sort
  remote:     block_files for ca38f237040d417466fcc1048ec6254f1ffdfb35: Blocked filename 'xplat/fbandroid/files' matched name pattern '^xplat/fbandroid/'. Rename or remove this file and try again.
  remote:     block_files for ca38f237040d417466fcc1048ec6254f1ffdfb35: Blocked filename 'xplat/fbcode/files' matched name pattern '^xplat/fbcode/'. Rename or remove this file and try again.
  remote:     block_files for ca38f237040d417466fcc1048ec6254f1ffdfb35: Blocked filename 'xplat/fbobjc/files' matched name pattern '^xplat/fbobjc/'. Rename or remove this file and try again.

Owners files are disallowed
  $ hg up -C -q "min(all())"
  $ echo 1 > OWNERS
  $ echo 1 > good
  $ hg add good OWNERS
  $ hg ci -m 'owners'
  $ hg push -r . --to master_bookmark
  pushing rev 4fb5a8e3ae8f to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 4fb5a8e3ae8f09634fbe63d0de903285ffe95471: Blocked filename 'OWNERS' matched name pattern '^OWNERS$'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

Owners files are disallowed
  $ hg up -C -q "min(all())"
  $ echo 1 > owners
  $ hg addremove
  adding owners
  $ hg ci -m 'owners'
  $ hg push -r . --to master_bookmark
  pushing rev 99177114cd07 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 99177114cd07e745baa4c4470f0c5d80e63ab3e1: Blocked filename 'owners' matched name pattern '^owners$'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

Owners files are disallowed
  $ hg up -C -q "min(all())"
  $ mkdir dir
  $ echo 1 > dir/owners
  $ hg addremove
  adding dir/owners
  $ hg ci -m 'owners'
  $ hg push -r . --to master_bookmark
  pushing rev b742a1aa7e76 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for b742a1aa7e76a2a3183aa36d4e53831578493b97: Blocked filename 'dir/owners' matched name pattern '.*/owners$'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

But file with owners inside is fine
  $ hg up -C -q "min(all())"
  $ mkdir dir
  $ echo 1 > dir/myowners
  $ echo 1 > dir/ownersmine
  $ echo 1 > ownersmine
  $ echo 1 > myowners
  $ hg -q addremove
  $ hg ci -m 'owners'
  $ hg push -r . --to master_bookmark
  pushing rev eb744398fce9 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Cannot commit .git stuff into the repo
  $ hg up -C -q "min(all())"
  $ mkdir -p dir/.git
  $ echo > dir/.git/HEAD
  $ hg -q addremove
  $ hg ci -m 'git'
  $ hg push -r . --to master_bookmark
  pushing rev 1bc4b64267b9 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 1bc4b64267b91310f557991f7667defebebf71be: Blocked filename 'dir/.git/HEAD' matched name pattern '/\.git/'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

Cannot commit .git stuff into the repo root
  $ hg up -C -q "min(all())"
  $ mkdir .git
  $ echo > .git/HEAD
  $ hg -q addremove
  $ hg ci -m 'git'
  $ hg push -r . --to master_bookmark
  pushing rev 5bec34e0a829 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 5bec34e0a82983a7897a7eac6e79e1bac2dee808: Blocked filename '.git/HEAD' matched name pattern '^\.git/'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

Something.git is ok
  $ hg up -C -q "min(all())"
  $ mkdir dir.git
  $ echo 1 > dir.git/woot
  $ hg -q addremove
  $ hg ci -m 'ok'
  $ hg push -r . --to master_bookmark
  pushing rev e80f6e0538ac to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

The dot in the [.]git hook pattern isn't a wildcard that matches any character
  $ hg up -C -q "min(all())"
  $ mkdir ogit
  $ echo 1 > ogit/woot
  $ hg -q addremove
  $ hg ci -m 'ok'
  $ hg push -r . --to master_bookmark
  pushing rev 644cd313ce02 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

.watchmanconfig file is allowed in the root, also not blocking other similar names
  $ hg up -q "min(all())"
  $ echo "{}" > .watchmanconfig
  $ mkdir -p dir
  $ echo "{}" > dir/.watchmanconfignot
  $ echo "{}" > .watchmanconfignot
  $ echo "{}" > dir/_watchmanconfig
  $ echo "{}" > _watchmanconfig
  $ echo "{}" > dir/not.watchmanconfig
  $ echo "{}" > not.watchmanconfig
  $ hg ci -Aqm ok
  $ hg push -r . --to master_bookmark
  pushing rev c882e4db1af1 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

.watchmanconfig file is not allowed in any subdir
  $ hg up -q "min(all())"
  $ mkdir -p dir
  $ echo "{}" > dir/.watchmanconfig
  $ hg ci -Aqm failure
  $ hg push -r . --to master_bookmark
  pushing rev d3b5973e930b to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for d3b5973e930bdf71e079b84f78a70bf747191279: Blocked filename 'dir/.watchmanconfig' matched name pattern '.*/\.watchmanconfig$'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

Check that can delete a file, which we cannot add
- first, add the file via a bypass commit string
  $ hg up -q "min(all())"
  $ echo 1 > OWNERS
  $ hg ci -qAm "add OWNERS @bypass_block_files"
  $ hg push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark

- now delete it without a bypass
  $ hg up -q remote/master_bookmark
  $ hg rm OWNERS
  $ hg ci -qm "delete owners"
  $ hg push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark

Pushing to experimental directories should not work
  $ hg up -q "min(all())"
  $ mkdir -p fbcode/experimental/this_is_a_test
  $ echo "test" > fbcode/experimental/this_is_a_test/file
  $ hg ci -Aqm experimental
  $ hg push -r . --to master_bookmark
  pushing rev 212a10abff88 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 212a10abff88450a2707b1d73b2e76c4fb1f2be6: Blocked filename 'fbcode/experimental/this_is_a_test/file' matched name pattern '^fbcode/experimental'. Rename or remove this file and try again.
  abort: unexpected EOL, expected netstring digit
  [255]

Deleting from experimental directories should still be allowed
- first, add an experimental directory via a bypass
  $ hg up -q "min(all())"
  $ mkdir -p fbcode/experimental/this_is_a_test
  $ echo "test" > fbcode/experimental/this_is_a_test/file
  $ hg ci -Aqm experimental
  $ hg push -r . --to master_bookmark --pushvar "TEST_BYPASS=true" 2>&1 | grep updating
  updating bookmark master_bookmark

- now delete it without a bypass
  $ hg up -q remote/master_bookmark
  $ hg rm fbcode/experimental/this_is_a_test/file
  $ hg ci -qm "delete this_is_a_test"
  $ hg push -r . --to master_bookmark
  pushing rev c3afe991fb3d to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
