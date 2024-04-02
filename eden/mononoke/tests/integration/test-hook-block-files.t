# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

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
  $ hgmn push -r . --to master_bookmark
  pushing rev 7de92e406b02 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
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
  $ hgmn push -r . --to master_bookmark
  pushing rev 94d93052245d to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
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
  $ hgmn push -r . --to master_bookmark
  pushing rev f8301844633b to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for f8301844633b60c2b4f8b990279394d831ab90c7: Blocked filename 'buck-out/file' matched name pattern '^buck-out/'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for f8301844633b60c2b4f8b990279394d831ab90c7: Blocked filename 'buck-out/file' matched name pattern '^buck-out/'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for f8301844633b60c2b4f8b990279394d831ab90c7: Blocked filename 'buck-out/file' matched name pattern '^buck-out/'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

buck-out directory is not allowed in any subdir
  $ hg up -q "min(all())"
  $ mkdir -p dir/buck-out
  $ echo "bad" > dir/buck-out/file
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev 409273951981 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 40927395198136c0dc65978d4fec6a8bf8386d4d: Blocked filename 'dir/buck-out/file' matched name pattern '/buck-out/'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for 40927395198136c0dc65978d4fec6a8bf8386d4d: Blocked filename 'dir/buck-out/file' matched name pattern '/buck-out/'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for 40927395198136c0dc65978d4fec6a8bf8386d4d: Blocked filename 'dir/buck-out/file' matched name pattern '/buck-out/'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED does the needful
  $ hg up -q "min(all())"
  $ echo "bad" > important_DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED.txt
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev d1a6e60539c6 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for d1a6e60539c6d4cd8df0c1fd442dcba98ef76bdf: Blocked filename 'important_DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED.txt' matched name pattern 'DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for d1a6e60539c6d4cd8df0c1fd442dcba98ef76bdf: Blocked filename 'important_DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED.txt' matched name pattern 'DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for d1a6e60539c6d4cd8df0c1fd442dcba98ef76bdf: Blocked filename 'important_DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED.txt' matched name pattern 'DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

Old fbmake leftovers cannot be committed
  $ hg up -q "min(all())"
  $ mkdir -p fbcode/_bin
  $ echo "bad" > fbcode/_bin/file
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  pushing rev 10b8f7a92bd1 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 10b8f7a92bd16630481eac34cac5b832edb9cb71: Blocked filename 'fbcode/_bin/file' matched name pattern '^fbcode/_bin/'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for 10b8f7a92bd16630481eac34cac5b832edb9cb71: Blocked filename 'fbcode/_bin/file' matched name pattern '^fbcode/_bin/'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for 10b8f7a92bd16630481eac34cac5b832edb9cb71: Blocked filename 'fbcode/_bin/file' matched name pattern '^fbcode/_bin/'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

Cannot nest project dirs badly
  $ hg up -q "min(all())"
  $ for path in fbandroid/fbandroid fbcode/fbcode fbobjc/fbobjc xplat/xplat; do
  > mkdir -p $path
  > echo fail > $path/files
  > done
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark |& grep -P 'remote:\s+block_files for' | sort
  remote:     block_files for 5d971d690977075710cf1270860e2ab65015eeec: Blocked filename 'fbandroid/fbandroid/files' matched name pattern '^fbandroid/fbandroid/'. Rename or remove this file and try again.
  remote:     block_files for 5d971d690977075710cf1270860e2ab65015eeec: Blocked filename 'fbandroid/fbandroid/files' matched name pattern '^fbandroid/fbandroid/'. Rename or remove this file and try again.
  remote:     block_files for 5d971d690977075710cf1270860e2ab65015eeec: Blocked filename 'fbcode/fbcode/files' matched name pattern '^fbcode/fbcode/'. Rename or remove this file and try again.
  remote:     block_files for 5d971d690977075710cf1270860e2ab65015eeec: Blocked filename 'fbcode/fbcode/files' matched name pattern '^fbcode/fbcode/'. Rename or remove this file and try again.
  remote:     block_files for 5d971d690977075710cf1270860e2ab65015eeec: Blocked filename 'fbobjc/fbobjc/files' matched name pattern '^fbobjc/fbobjc/'. Rename or remove this file and try again.
  remote:     block_files for 5d971d690977075710cf1270860e2ab65015eeec: Blocked filename 'fbobjc/fbobjc/files' matched name pattern '^fbobjc/fbobjc/'. Rename or remove this file and try again.
  remote:     block_files for 5d971d690977075710cf1270860e2ab65015eeec: Blocked filename 'xplat/xplat/files' matched name pattern '^xplat/xplat/'. Rename or remove this file and try again.
  remote:     block_files for 5d971d690977075710cf1270860e2ab65015eeec: Blocked filename 'xplat/xplat/files' matched name pattern '^xplat/xplat/'. Rename or remove this file and try again.

Cannot put crud in xplat
  $ hg up -q "min(all())"
  $ for path in xplat/fbandroid xplat/fbcode xplat/fbobjc; do
  > mkdir -p $path
  > echo fail > $path/files
  > done
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark |& grep -P 'remote:\s+block_files for' | sort
  remote:     block_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Blocked filename 'xplat/fbandroid/files' matched name pattern '^xplat/fbandroid/'. Rename or remove this file and try again.
  remote:     block_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Blocked filename 'xplat/fbandroid/files' matched name pattern '^xplat/fbandroid/'. Rename or remove this file and try again.
  remote:     block_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Blocked filename 'xplat/fbcode/files' matched name pattern '^xplat/fbcode/'. Rename or remove this file and try again.
  remote:     block_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Blocked filename 'xplat/fbcode/files' matched name pattern '^xplat/fbcode/'. Rename or remove this file and try again.
  remote:     block_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Blocked filename 'xplat/fbobjc/files' matched name pattern '^xplat/fbobjc/'. Rename or remove this file and try again.
  remote:     block_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Blocked filename 'xplat/fbobjc/files' matched name pattern '^xplat/fbobjc/'. Rename or remove this file and try again.

Owners files are disallowed
  $ hg up -C -q "min(all())"
  $ echo 1 > OWNERS
  $ echo 1 > good
  $ hg add good OWNERS
  $ hg ci -m 'owners'
  $ hgmn push -r . --to master_bookmark
  pushing rev fb86edb43149 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for fb86edb431492c799bfb61c60137272899084b19: Blocked filename 'OWNERS' matched name pattern '^OWNERS$'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for fb86edb431492c799bfb61c60137272899084b19: Blocked filename 'OWNERS' matched name pattern '^OWNERS$'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for fb86edb431492c799bfb61c60137272899084b19: Blocked filename 'OWNERS' matched name pattern '^OWNERS$'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

Owners files are disallowed
  $ hg up -C -q "min(all())"
  $ echo 1 > owners
  $ hg addremove
  adding owners
  $ hg ci -m 'owners'
  $ hgmn push -r . --to master_bookmark
  pushing rev a2511fd15afb to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for a2511fd15afb17f7227bcf57c4313cb38b48d246: Blocked filename 'owners' matched name pattern '^owners$'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for a2511fd15afb17f7227bcf57c4313cb38b48d246: Blocked filename 'owners' matched name pattern '^owners$'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for a2511fd15afb17f7227bcf57c4313cb38b48d246: Blocked filename 'owners' matched name pattern '^owners$'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

Owners files are disallowed
  $ hg up -C -q "min(all())"
  $ mkdir dir
  $ echo 1 > dir/owners
  $ hg addremove
  adding dir/owners
  $ hg ci -m 'owners'
  $ hgmn push -r . --to master_bookmark
  pushing rev 8d6b816aa4f3 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 8d6b816aa4f370227f2b8263e630aa0e15b3d1a3: Blocked filename 'dir/owners' matched name pattern '.*/owners$'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for 8d6b816aa4f370227f2b8263e630aa0e15b3d1a3: Blocked filename 'dir/owners' matched name pattern '.*/owners$'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for 8d6b816aa4f370227f2b8263e630aa0e15b3d1a3: Blocked filename 'dir/owners' matched name pattern '.*/owners$'. Rename or remove this file and try again."
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
  $ hgmn push -r . --to master_bookmark
  pushing rev c5cf7687c442 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
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
  $ hgmn push -r . --to master_bookmark
  pushing rev ad3e5bd3f2e3 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for ad3e5bd3f2e3084dfd3c18dde9fc4d3642fc2637: Blocked filename 'dir/.git/HEAD' matched name pattern '/\.git/'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for ad3e5bd3f2e3084dfd3c18dde9fc4d3642fc2637: Blocked filename 'dir/.git/HEAD' matched name pattern '/\.git/'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for ad3e5bd3f2e3084dfd3c18dde9fc4d3642fc2637: Blocked filename 'dir/.git/HEAD' matched name pattern '/\\.git/'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

Cannot commit .git stuff into the repo root
  $ hg up -C -q "min(all())"
  $ mkdir .git
  $ echo > .git/HEAD
  $ hg -q addremove
  $ hg ci -m 'git'
  $ hgmn push -r . --to master_bookmark
  pushing rev cba7ce32b8b5 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for cba7ce32b8b5ad219c7a5610933ec7d05d3f47b6: Blocked filename '.git/HEAD' matched name pattern '^\.git/'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for cba7ce32b8b5ad219c7a5610933ec7d05d3f47b6: Blocked filename '.git/HEAD' matched name pattern '^\.git/'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for cba7ce32b8b5ad219c7a5610933ec7d05d3f47b6: Blocked filename '.git/HEAD' matched name pattern '^\\.git/'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

Something.git is ok
  $ hg up -C -q "min(all())"
  $ mkdir dir.git
  $ echo 1 > dir.git/woot
  $ hg -q addremove
  $ hg ci -m 'ok'
  $ hgmn push -r . --to master_bookmark
  pushing rev 6e5252e4e930 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
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
  $ hgmn push -r . --to master_bookmark
  pushing rev d47a144a1dc1 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
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
  $ hgmn push -r . --to master_bookmark
  pushing rev 42d8ce9969fa to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
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
  $ hgmn push -r . --to master_bookmark
  pushing rev 95659407febd to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 95659407febd3cf89a15cfbfb04eff3ae6b3d23b: Blocked filename 'dir/.watchmanconfig' matched name pattern '.*/\.watchmanconfig$'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for 95659407febd3cf89a15cfbfb04eff3ae6b3d23b: Blocked filename 'dir/.watchmanconfig' matched name pattern '.*/\.watchmanconfig$'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for 95659407febd3cf89a15cfbfb04eff3ae6b3d23b: Blocked filename 'dir/.watchmanconfig' matched name pattern '.*/\\.watchmanconfig$'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

Check that can delete a file, which we cannot add
- first, add the file via a bypass commit string
  $ hg up -q "min(all())"
  $ echo 1 > OWNERS
  $ hg ci -qAm "add OWNERS @bypass_block_files"
  $ hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark

- now delete it without a bypass
  $ hgmn up -q default/master_bookmark
  $ hg rm OWNERS
  $ hg ci -qm "delete owners"
  $ hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark

Pushing to experimental directories should not work
  $ hg up -q "min(all())"
  $ mkdir -p fbcode/experimental/this_is_a_test
  $ echo "test" > fbcode/experimental/this_is_a_test/file
  $ hg ci -Aqm experimental
  $ hgmn push -r . --to master_bookmark
  pushing rev 164dce8b98ef to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_files for 164dce8b98ef7d21f81b57bf868d14a33acc84ee: Blocked filename 'fbcode/experimental/this_is_a_test/file' matched name pattern '^fbcode/experimental'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_files for 164dce8b98ef7d21f81b57bf868d14a33acc84ee: Blocked filename 'fbcode/experimental/this_is_a_test/file' matched name pattern '^fbcode/experimental'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_files for 164dce8b98ef7d21f81b57bf868d14a33acc84ee: Blocked filename 'fbcode/experimental/this_is_a_test/file' matched name pattern '^fbcode/experimental'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

Deleting from experimental directories should still be allowed
- first, add an experimental directory via a bypass
  $ hg up -q "min(all())"
  $ mkdir -p fbcode/experimental/this_is_a_test
  $ echo "test" > fbcode/experimental/this_is_a_test/file
  $ hg ci -Aqm experimental
  $ hgmn push -r . --to master_bookmark --pushvar "TEST_BYPASS=true" 2>&1 | grep updating
  updating bookmark master_bookmark

- now delete it without a bypass
  $ hgmn up -q default/master_bookmark
  $ hg rm fbcode/experimental/this_is_a_test/file
  $ hg ci -qm "delete this_is_a_test"
  $ hgmn push -r . --to master_bookmark
  pushing rev 8592809d2854 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
