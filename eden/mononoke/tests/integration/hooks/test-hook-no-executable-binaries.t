# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ export LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 LANGUAGE=en_US.UTF-8

  $ hook_test_setup \
  > no_executable_binaries <(
  >   cat <<CONF
  > config_json='''{
  >   "illegal_executable_binary_message": "Executable file \${filename} can't be committed."
  > }'''
  > CONF
  > )

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Ok file path - should work

  $ touch normal_file
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev dd7648b00878 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Executable script - should work

  $ hg up -q "min(all())"
  $ touch script.sh
  $ chmod +x script.sh
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 80f52f5bb249 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Executable binary - should fail 

  $ hg up -q "min(all())"
  $ echo -e "\x00\x12\x34\x56\x78" > binary_file.exe
  $ chmod +x binary_file.exe
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 2738cc1d1b73 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_executable_binaries for 2738cc1d1b73a4e6e196f8f2075c42e24e8f3abf: Executable file binary_file.exe can't be committed.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_executable_binaries for 2738cc1d1b73a4e6e196f8f2075c42e24e8f3abf: Executable file binary_file.exe can't be committed.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_executable_binaries for 2738cc1d1b73a4e6e196f8f2075c42e24e8f3abf: Executable file binary_file.exe can't be committed."
  abort: unexpected EOL, expected netstring digit
  [255]

Non-executable binary file - should work

  $ hg up -q "min(all())"
  $ echo -e "\x00\x12\x34\x56\x78" > binary_file
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 93f08e97efc1 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark


Executable binary under specific directory - should fail

  $ hg up -q "min(all())"
  $ mkdir some_dir
  $ echo -e "\x00\x12\x34\x56\x78" > some_dir/binary_file.exe
  $ chmod +x some_dir/binary_file.exe
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 03e66567b425 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_executable_binaries for 03e66567b4257e9891da6db09f751d726a274fa9: Executable file some_dir/binary_file.exe can't be committed.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     no_executable_binaries for 03e66567b4257e9891da6db09f751d726a274fa9: Executable file some_dir/binary_file.exe can't be committed.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nno_executable_binaries for 03e66567b4257e9891da6db09f751d726a274fa9: Executable file some_dir/binary_file.exe can't be committed."
  abort: unexpected EOL, expected netstring digit
  [255]

-- Add `some_dir` path to the config's allow-list
  $ hook_test_setup \
  > no_executable_binaries <(
  >   cat <<CONF
  > config_json='''{
  >   "illegal_executable_binary_message": "Executable file \${filename} can't be committed.",
  >   "allow_list_paths": ["some_dir"]
  > }'''
  > CONF
  > )
  abort: repository `$TESTTMP/repo-hg` already exists
  abort: destination 'repo2' is not empty
  $ force_update_configerator

Executable binary under allow-listed directory - should pass

  $ hgmn push -r . --to master_bookmark
  pushing rev 03e66567b425 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
