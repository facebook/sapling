# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup \
  > conflict_markers <(
  >   echo 'bypass_commit_string="@ignore-conflict-markers"'
  > )

Negative testing
  $ markers_good=('<<<<<<<'
  > '<<<<<<<<<<'
  > '>>>>>>>'
  > '<<<<<<<'
  > '>>>>>>>>>>'
  > '====='
  > '===============')
  $ hg up -q "min(all())"

  $ i=0
  $ for input in "${markers_good[@]}"; do
  >  i=$((i+1))
  >  printf "$input" > "file$i"
  > done
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 069fca863ff8 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Positive testing
  $ hg up -q "min(all())"
  $ echo '>>>>>>> 123' > 1 && hg add 1 && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 17a746afd78e to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     conflict_markers for 17a746afd78ed3f7f06d1d5396fa89adf656ae51: Conflict markers were found in file '1'
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     conflict_markers for 17a746afd78ed3f7f06d1d5396fa89adf656ae51: Conflict markers were found in file '1'
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nconflict_markers for 17a746afd78ed3f7f06d1d5396fa89adf656ae51: Conflict markers were found in file '1'"
  abort: unexpected EOL, expected netstring digit
  [255]

  $ hg up -q "min(all())"
  $ echo '<<<<<<< 123' > 1 && hg add 1 && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 948f2ceaf570 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     conflict_markers for 948f2ceaf570f89539966000cf65d4a56dc4ec37: Conflict markers were found in file '1'
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     conflict_markers for 948f2ceaf570f89539966000cf65d4a56dc4ec37: Conflict markers were found in file '1'
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nconflict_markers for 948f2ceaf570f89539966000cf65d4a56dc4ec37: Conflict markers were found in file '1'"
  abort: unexpected EOL, expected netstring digit
  [255]

Negative testing
Files with bad markers should be accepted with these suffixes
  $ hg up -q "min(all())"
  $ suffixes=('.md' '.markdown' '.rdoc' '.rst')
  $ for suffix in "${suffixes[@]}"; do
  $     echo ">>>>>>> " > "file$suffix"
  $ done
  $ hg ci -Aqm 'markdowns'
  $ hgmn push -r . --to master_bookmark
  pushing rev ced9269b0dde to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Negative Testing
Files with bad markers should be accepted if they are binary.
File is considered binary if it contains \0
  $ hg up -q "min(all())"
  $ echo -e ">>>>>>> \0" > file
  $ hg ci -Aqm binary
  $ hgmn push -r . --to master_bookmark
  pushing rev e913daf3ef9f to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Test bypass
  $ hg up -q "min(all())"
  $ echo -e ">>>>>>> " > largefile
  $ hg ci -Aqm '@ignore-conflict-markers'
  $ hgmn push -r . --to master_bookmark
  pushing rev a45fdf76c250 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Test markers not on the first line
  $ hg up -q "min(all())"
  $ echo -e "ololo\nonemore\n\n>>>>>>> " > notfirstline
  $ hg ci -Aqm notfirstline
  $ hgmn push -r . --to master_bookmark
  pushing rev be491e50f486 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     conflict_markers for be491e50f4868f90970fb2267d7724d8580780af: Conflict markers were found in file 'notfirstline'
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     conflict_markers for be491e50f4868f90970fb2267d7724d8580780af: Conflict markers were found in file 'notfirstline'
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nconflict_markers for be491e50f4868f90970fb2267d7724d8580780af: Conflict markers were found in file 'notfirstline'"
  abort: unexpected EOL, expected netstring digit
  [255]
