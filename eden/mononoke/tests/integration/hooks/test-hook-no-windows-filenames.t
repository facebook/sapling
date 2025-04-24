# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ export LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 LANGUAGE=en_US.UTF-8

  $ hook_test_setup no_windows_filenames <( \
  >   cat <<CONF
  > bypass_pushvar="ALLOW_BAD_WINDOWS_FILENAMES=true"
  > config_json='''{
  >   "allowed_paths": "^fbcode/videoinfra|^fbcode/transient_analysis|^fbcode/tupperware|^fbcode/realtime|^fbcode/npe|^fbcode/axon|^fbcode/ame|^third-party/rpms|^opsfiles/|^fbobjc/Libraries/Lexical/",
  >   "illegal_filename_message": "ABORT: Illegal windows filename: \${filename}. Name and path of file in windows should not match regex \${illegal_pattern}"  
  > }'''
  > CONF
  > ) 

  $ hg up -q "min(all())"
  $ echo "ok"  > "com"
  $ hg ci -Aqm success
  $ hg push -r . --to master_bookmark
  pushing rev 8c52c327cb35 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

  $ hg up -q "min(all())"
  $ echo "bad" > "COM5"
  $ hg ci -Aqm failure
  warning: filename contains 'COM5', which is reserved on Windows: COM5
  $ hg push -r . --to master_bookmark
  pushing rev 6dfbf17e5980 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_windows_filenames for 6dfbf17e598069dbfd793b9ab7ce26f28956704e: ABORT: Illegal windows filename: COM5. Name and path of file in windows should not match regex (^(?i)((((com|lpt)\d)|con|prn|aux|nul))($|\.))|<|>|:|"|/|\\|\||\?|\*|[\x00-\x1F]|(\.| )$
  abort: unexpected EOL, expected netstring digit
  [255]

  $ hg up -q "min(all())"
  $ echo "bad" > "nul.txt"
  $ hg ci -Aqm failure
  warning: filename contains 'nul', which is reserved on Windows: nul.txt
  $ hg push -r . --to master_bookmark
  pushing rev df11f23e77f0 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_windows_filenames for df11f23e77f014da530ce7e11942f12cfb7645c6: ABORT: Illegal windows filename: nul.txt. Name and path of file in windows should not match regex (^(?i)((((com|lpt)\d)|con|prn|aux|nul))($|\.))|<|>|:|"|/|\\|\||\?|\*|[\x00-\x1F]|(\.| )$
  abort: unexpected EOL, expected netstring digit
  [255]

  $ hg up -q "min(all())"
  $ mkdir dir
  $ echo "bad" > dir/CoN.txt
  $ hg ci -Aqm failure
  warning: filename contains 'CoN', which is reserved on Windows: dir/CoN.txt
  $ hg push -r . --to master_bookmark
  pushing rev 3e9105459e59 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_windows_filenames for 3e9105459e595067752892deb360c438dcb92351: ABORT: Illegal windows filename: dir/CoN.txt. Name and path of file in windows should not match regex (^(?i)((((com|lpt)\d)|con|prn|aux|nul))($|\.))|<|>|:|"|/|\\|\||\?|\*|[\x00-\x1F]|(\.| )$
  abort: unexpected EOL, expected netstring digit
  [255]

  $ hg up -q "min(all())"
  $ mkdir dir
  $ echo "ok" > dir/Icon.txt
  $ hg ci -Aqm success
  $ hg push -r . --to master_bookmark
  pushing rev 31fccd21dd5a to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

  $ hg up -q "min(all())"
  $ mkdir dir
  $ echo "ok" > dir/Icom5
  $ hg ci -Aqm success
  $ hg push -r . --to master_bookmark
  pushing rev e268f3d11d0d to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

  $ hg up -q "min(all())"
  $ mkdir con
  $ echo "bad" > con/foo
  $ hg ci -Aqm failure
  warning: filename contains 'con', which is reserved on Windows: con/foo
  $ hg push -r . --to master_bookmark
  pushing rev 10c460e53000 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     no_windows_filenames for 10c460e53000520521df87f44c12020fa6c65ad5: ABORT: Illegal windows filename: con/foo. Name and path of file in windows should not match regex (^(?i)((((com|lpt)\d)|con|prn|aux|nul))($|\.))|<|>|:|"|/|\\|\||\?|\*|[\x00-\x1F]|(\.| )$
  abort: unexpected EOL, expected netstring digit
  [255]
