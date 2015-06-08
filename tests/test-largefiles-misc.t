This file contains testcases that tend to be related to special cases or less
common commands affecting largefile.

Each sections should be independent of each others.

  $ USERCACHE="$TESTTMP/cache"; export USERCACHE
  $ mkdir "${USERCACHE}"
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > largefiles=
  > purge=
  > rebase=
  > transplant=
  > [phases]
  > publish=False
  > [largefiles]
  > minsize=2
  > patterns=glob:**.dat
  > usercache=${USERCACHE}
  > [hooks]
  > precommit=sh -c "echo \\"Invoking status precommit hook\\"; hg status"
  > EOF



Test copies and moves from a directory other than root (issue3516)
=========================================================================

  $ hg init lf_cpmv
  $ cd lf_cpmv
  $ mkdir dira
  $ mkdir dira/dirb
  $ touch dira/dirb/largefile
  $ hg add --large dira/dirb/largefile
  $ hg commit -m "added"
  Invoking status precommit hook
  A dira/dirb/largefile
  $ cd dira
  $ hg cp dirb/largefile foo/largefile

TODO: Ideally, this should mention the largefile, not the standin
  $ hg log -T '{rev}\n' --stat 'set:clean()'
  0
   .hglf/dira/dirb/largefile |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg ci -m "deep copy"
  Invoking status precommit hook
  A dira/foo/largefile
  $ find . | sort
  .
  ./dirb
  ./dirb/largefile
  ./foo
  ./foo/largefile
  $ hg mv foo/largefile baz/largefile
  $ hg ci -m "moved"
  Invoking status precommit hook
  A dira/baz/largefile
  R dira/foo/largefile
  $ find . | sort
  .
  ./baz
  ./baz/largefile
  ./dirb
  ./dirb/largefile
  $ cd ..
  $ hg mv dira dirc
  moving .hglf/dira/baz/largefile to .hglf/dirc/baz/largefile (glob)
  moving .hglf/dira/dirb/largefile to .hglf/dirc/dirb/largefile (glob)
  $ find * | sort
  dirc
  dirc/baz
  dirc/baz/largefile
  dirc/dirb
  dirc/dirb/largefile

  $ hg clone -q . ../fetch
  $ hg --config extensions.fetch= fetch ../fetch
  abort: uncommitted changes
  [255]
  $ hg up -qC
  $ cd ..

Clone a local repository owned by another user
===================================================

#if unix-permissions

We have to simulate that here by setting $HOME and removing write permissions
  $ ORIGHOME="$HOME"
  $ mkdir alice
  $ HOME="`pwd`/alice"
  $ cd alice
  $ hg init pubrepo
  $ cd pubrepo
  $ dd if=/dev/zero bs=1k count=11k > a-large-file 2> /dev/null
  $ hg add --large a-large-file
  $ hg commit -m "Add a large file"
  Invoking status precommit hook
  A a-large-file
  $ cd ..
  $ chmod -R a-w pubrepo
  $ cd ..
  $ mkdir bob
  $ HOME="`pwd`/bob"
  $ cd bob
  $ hg clone --pull ../alice/pubrepo pubrepo
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..
  $ chmod -R u+w alice/pubrepo
  $ HOME="$ORIGHOME"

#endif


Symlink to a large largefile should behave the same as a symlink to a normal file
=====================================================================================

#if symlink

  $ hg init largesymlink
  $ cd largesymlink
  $ dd if=/dev/zero bs=1k count=10k of=largefile 2>/dev/null
  $ hg add --large largefile
  $ hg commit -m "commit a large file"
  Invoking status precommit hook
  A largefile
  $ ln -s largefile largelink
  $ hg add largelink
  $ hg commit -m "commit a large symlink"
  Invoking status precommit hook
  A largelink
  $ rm -f largelink
  $ hg up >/dev/null
  $ test -f largelink
  [1]
  $ test -L largelink
  [1]
  $ rm -f largelink # make next part of the test independent of the previous
  $ hg up -C >/dev/null
  $ test -f largelink
  $ test -L largelink
  $ cd ..

#endif


test for pattern matching on 'hg status':
==============================================


to boost performance, largefiles checks whether specified patterns are
related to largefiles in working directory (NOT to STANDIN) or not.

  $ hg init statusmatch
  $ cd statusmatch

  $ mkdir -p a/b/c/d
  $ echo normal > a/b/c/d/e.normal.txt
  $ hg add a/b/c/d/e.normal.txt
  $ echo large > a/b/c/d/e.large.txt
  $ hg add --large a/b/c/d/e.large.txt
  $ mkdir -p a/b/c/x
  $ echo normal > a/b/c/x/y.normal.txt
  $ hg add a/b/c/x/y.normal.txt
  $ hg commit -m 'add files'
  Invoking status precommit hook
  A a/b/c/d/e.large.txt
  A a/b/c/d/e.normal.txt
  A a/b/c/x/y.normal.txt

(1) no pattern: no performance boost
  $ hg status -A
  C a/b/c/d/e.large.txt
  C a/b/c/d/e.normal.txt
  C a/b/c/x/y.normal.txt

(2) pattern not related to largefiles: performance boost
  $ hg status -A a/b/c/x
  C a/b/c/x/y.normal.txt

(3) pattern related to largefiles: no performance boost
  $ hg status -A a/b/c/d
  C a/b/c/d/e.large.txt
  C a/b/c/d/e.normal.txt

(4) pattern related to STANDIN (not to largefiles): performance boost
  $ hg status -A .hglf/a
  C .hglf/a/b/c/d/e.large.txt

(5) mixed case: no performance boost
  $ hg status -A a/b/c/x a/b/c/d
  C a/b/c/d/e.large.txt
  C a/b/c/d/e.normal.txt
  C a/b/c/x/y.normal.txt

verify that largefiles doesn't break filesets

  $ hg log --rev . --exclude "set:binary()"
  changeset:   0:41bd42f10efa
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add files
  
verify that large files in subrepos handled properly
  $ hg init subrepo
  $ echo "subrepo = subrepo" > .hgsub
  $ hg add .hgsub
  $ hg ci -m "add subrepo"
  Invoking status precommit hook
  A .hgsub
  ? .hgsubstate
  $ echo "rev 1" > subrepo/large.txt
  $ hg add --large subrepo/large.txt
  $ hg sum
  parent: 1:8ee150ea2e9c tip
   add subrepo
  branch: default
  commit: 1 subrepos
  update: (current)
  phases: 2 draft
  $ hg st
  $ hg st -S
  A subrepo/large.txt
  $ hg ci -S -m "commit top repo"
  committing subrepository subrepo
  Invoking status precommit hook
  A large.txt
  Invoking status precommit hook
  M .hgsubstate
# No differences
  $ hg st -S
  $ hg sum
  parent: 2:ce4cd0c527a6 tip
   commit top repo
  branch: default
  commit: (clean)
  update: (current)
  phases: 3 draft
  $ echo "rev 2" > subrepo/large.txt
  $ hg st -S
  M subrepo/large.txt
  $ hg sum
  parent: 2:ce4cd0c527a6 tip
   commit top repo
  branch: default
  commit: 1 subrepos
  update: (current)
  phases: 3 draft
  $ hg ci -m "this commit should fail without -S"
  abort: uncommitted changes in subrepository 'subrepo'
  (use --subrepos for recursive commit)
  [255]

Add a normal file to the subrepo, then test archiving

  $ echo 'normal file' > subrepo/normal.txt
  $ touch large.dat
  $ mv subrepo/large.txt subrepo/renamed-large.txt
  $ hg addremove -S --dry-run
  adding large.dat as a largefile
  removing subrepo/large.txt
  adding subrepo/normal.txt
  adding subrepo/renamed-large.txt
  $ hg status -S
  ! subrepo/large.txt
  ? large.dat
  ? subrepo/normal.txt
  ? subrepo/renamed-large.txt

  $ hg addremove --dry-run subrepo
  removing subrepo/large.txt (glob)
  adding subrepo/normal.txt (glob)
  adding subrepo/renamed-large.txt (glob)
  $ hg status -S
  ! subrepo/large.txt
  ? large.dat
  ? subrepo/normal.txt
  ? subrepo/renamed-large.txt
  $ cd ..

  $ hg -R statusmatch addremove --dry-run statusmatch/subrepo
  removing statusmatch/subrepo/large.txt (glob)
  adding statusmatch/subrepo/normal.txt (glob)
  adding statusmatch/subrepo/renamed-large.txt (glob)
  $ hg -R statusmatch status -S
  ! subrepo/large.txt
  ? large.dat
  ? subrepo/normal.txt
  ? subrepo/renamed-large.txt

  $ hg -R statusmatch addremove --dry-run -S
  adding large.dat as a largefile
  removing subrepo/large.txt
  adding subrepo/normal.txt
  adding subrepo/renamed-large.txt
  $ cd statusmatch

  $ mv subrepo/renamed-large.txt subrepo/large.txt
  $ hg addremove subrepo
  adding subrepo/normal.txt (glob)
  $ hg forget subrepo/normal.txt

  $ hg addremove -S
  adding large.dat as a largefile
  adding subrepo/normal.txt
  $ rm large.dat

  $ hg addremove subrepo
  $ hg addremove -S
  removing large.dat

Lock in subrepo, otherwise the change isn't archived

  $ hg ci -S -m "add normal file to top level"
  committing subrepository subrepo
  Invoking status precommit hook
  M large.txt
  A normal.txt
  Invoking status precommit hook
  M .hgsubstate
  $ hg archive -S ../lf_subrepo_archive
  $ find ../lf_subrepo_archive | sort
  ../lf_subrepo_archive
  ../lf_subrepo_archive/.hg_archival.txt
  ../lf_subrepo_archive/.hgsub
  ../lf_subrepo_archive/.hgsubstate
  ../lf_subrepo_archive/a
  ../lf_subrepo_archive/a/b
  ../lf_subrepo_archive/a/b/c
  ../lf_subrepo_archive/a/b/c/d
  ../lf_subrepo_archive/a/b/c/d/e.large.txt
  ../lf_subrepo_archive/a/b/c/d/e.normal.txt
  ../lf_subrepo_archive/a/b/c/x
  ../lf_subrepo_archive/a/b/c/x/y.normal.txt
  ../lf_subrepo_archive/subrepo
  ../lf_subrepo_archive/subrepo/large.txt
  ../lf_subrepo_archive/subrepo/normal.txt
  $ cat ../lf_subrepo_archive/.hg_archival.txt
  repo: 41bd42f10efa43698cc02052ea0977771cba506d
  node: d56a95e6522858bc08a724c4fe2bdee066d1c30b
  branch: default
  latesttag: null
  latesttagdistance: 4
  changessincelatesttag: 4

Test update with subrepos.

  $ hg update 0
  getting changed largefiles
  0 largefiles updated, 1 removed
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg status -S
  $ hg update tip
  getting changed largefiles
  1 largefiles updated, 0 removed
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status -S
# modify a large file
  $ echo "modified" > subrepo/large.txt
  $ hg st -S
  M subrepo/large.txt
# update -C should revert the change.
  $ hg update -C
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status -S

  $ hg forget -v subrepo/large.txt
  removing subrepo/large.txt (glob)

Test reverting a forgotten file
  $ hg revert -R subrepo subrepo/large.txt
  $ hg status -SA subrepo/large.txt
  C subrepo/large.txt

  $ hg rm -v subrepo/large.txt
  removing subrepo/large.txt (glob)
  $ hg revert -R subrepo subrepo/large.txt
  $ rm subrepo/large.txt
  $ hg addremove -S
  removing subrepo/large.txt
  $ hg st -S
  R subrepo/large.txt

Test archiving a revision that references a subrepo that is not yet
cloned (see test-subrepo-recursion.t):

  $ hg clone -U . ../empty
  $ cd ../empty
  $ hg archive --subrepos -r tip ../archive.tar.gz
  cloning subrepo subrepo from $TESTTMP/statusmatch/subrepo
  $ cd ..






Test addremove, forget and others
==============================================

Test that addremove picks up largefiles prior to the initial commit (issue3541)

  $ hg init addrm2
  $ cd addrm2
  $ touch large.dat
  $ touch large2.dat
  $ touch normal
  $ hg add --large large.dat
  $ hg addremove -v
  adding large2.dat as a largefile
  adding normal

Test that forgetting all largefiles reverts to islfilesrepo() == False
(addremove will add *.dat as normal files now)
  $ hg forget large.dat
  $ hg forget large2.dat
  $ hg addremove -v
  adding large.dat
  adding large2.dat

Test commit's addremove option prior to the first commit
  $ hg forget large.dat
  $ hg forget large2.dat
  $ hg add --large large.dat
  $ hg ci -Am "commit"
  adding large2.dat as a largefile
  Invoking status precommit hook
  A large.dat
  A large2.dat
  A normal
  $ find .hglf | sort
  .hglf
  .hglf/large.dat
  .hglf/large2.dat

Test actions on largefiles using relative paths from subdir

  $ mkdir sub
  $ cd sub
  $ echo anotherlarge > anotherlarge
  $ hg add --large anotherlarge
  $ hg st
  A sub/anotherlarge
  $ hg st anotherlarge
  A anotherlarge
  $ hg commit -m anotherlarge anotherlarge
  Invoking status precommit hook
  A sub/anotherlarge
  $ hg log anotherlarge
  changeset:   1:9627a577c5e9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     anotherlarge
  
  $ hg --debug log -T '{rev}: {desc}\n' ../sub/anotherlarge
  updated patterns: ['../.hglf/sub/../sub/anotherlarge', '../sub/anotherlarge']
  1: anotherlarge

  $ hg log -G anotherlarge
  @  changeset:   1:9627a577c5e9
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     anotherlarge
  |

  $ hg log glob:another*
  changeset:   1:9627a577c5e9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     anotherlarge
  
  $ hg --debug log -T '{rev}: {desc}\n' -G glob:another*
  updated patterns: ['glob:../.hglf/sub/another*', 'glob:another*']
  @  1: anotherlarge
  |

#if no-msys
  $ hg --debug log -T '{rev}: {desc}\n' 'glob:../.hglf/sub/another*' # no-msys
  updated patterns: ['glob:../.hglf/sub/another*']
  1: anotherlarge

  $ hg --debug log -G -T '{rev}: {desc}\n' 'glob:../.hglf/sub/another*' # no-msys
  updated patterns: ['glob:../.hglf/sub/another*']
  @  1: anotherlarge
  |
#endif

  $ echo more >> anotherlarge
  $ hg st .
  M anotherlarge
  $ hg cat anotherlarge
  anotherlarge
  $ hg revert anotherlarge
  $ hg st
  ? sub/anotherlarge.orig
  $ cd ..

Test glob logging from the root dir
  $ hg log glob:**another*
  changeset:   1:9627a577c5e9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     anotherlarge
  
  $ hg log -G glob:**another*
  @  changeset:   1:9627a577c5e9
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     anotherlarge
  |

  $ cd ..

Log from outer space
  $ hg --debug log -R addrm2 -T '{rev}: {desc}\n' 'addrm2/sub/anotherlarge'
  updated patterns: ['addrm2/.hglf/sub/anotherlarge', 'addrm2/sub/anotherlarge']
  1: anotherlarge
  $ hg --debug log -R addrm2 -T '{rev}: {desc}\n' 'addrm2/.hglf/sub/anotherlarge'
  updated patterns: ['addrm2/.hglf/sub/anotherlarge']
  1: anotherlarge


Check error message while exchange
=========================================================

issue3651: summary/outgoing with largefiles shows "no remote repo"
unexpectedly

  $ mkdir issue3651
  $ cd issue3651

  $ hg init src
  $ echo a > src/a
  $ hg -R src add --large src/a
  $ hg -R src commit -m '#0'
  Invoking status precommit hook
  A a

check messages when no remote repository is specified:
"no remote repo" route for "hg outgoing --large" is not tested here,
because it can't be reproduced easily.

  $ hg init clone1
  $ hg -R clone1 -q pull src
  $ hg -R clone1 -q update
  $ hg -R clone1 paths | grep default
  [1]

  $ hg -R clone1 summary --large
  parent: 0:fc0bd45326d3 tip
   #0
  branch: default
  commit: (clean)
  update: (current)
  phases: 1 draft
  largefiles: (no remote repo)

check messages when there is no files to upload:

  $ hg -q clone src clone2
  $ hg -R clone2 paths | grep default
  default = $TESTTMP/issue3651/src (glob)

  $ hg -R clone2 summary --large
  parent: 0:fc0bd45326d3 tip
   #0
  branch: default
  commit: (clean)
  update: (current)
  phases: 1 draft
  largefiles: (no files to upload)
  $ hg -R clone2 outgoing --large
  comparing with $TESTTMP/issue3651/src (glob)
  searching for changes
  no changes found
  largefiles: no files to upload
  [1]

  $ hg -R clone2 outgoing --large --graph --template "{rev}"
  comparing with $TESTTMP/issue3651/src (glob)
  searching for changes
  no changes found
  largefiles: no files to upload

check messages when there are files to upload:

  $ echo b > clone2/b
  $ hg -R clone2 add --large clone2/b
  $ hg -R clone2 commit -m '#1'
  Invoking status precommit hook
  A b
  $ hg -R clone2 summary --large
  parent: 1:1acbe71ce432 tip
   #1
  branch: default
  commit: (clean)
  update: (current)
  phases: 2 draft
  largefiles: 1 entities for 1 files to upload
  $ hg -R clone2 outgoing --large
  comparing with $TESTTMP/issue3651/src (glob)
  searching for changes
  changeset:   1:1acbe71ce432
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     #1
  
  largefiles to upload (1 entities):
  b
  
  $ hg -R clone2 outgoing --large --graph --template "{rev}"
  comparing with $TESTTMP/issue3651/src (glob)
  searching for changes
  @  1
  
  largefiles to upload (1 entities):
  b
  

  $ cp clone2/b clone2/b1
  $ cp clone2/b clone2/b2
  $ hg -R clone2 add --large clone2/b1 clone2/b2
  $ hg -R clone2 commit -m '#2: add largefiles referring same entity'
  Invoking status precommit hook
  A b1
  A b2
  $ hg -R clone2 summary --large
  parent: 2:6095d0695d70 tip
   #2: add largefiles referring same entity
  branch: default
  commit: (clean)
  update: (current)
  phases: 3 draft
  largefiles: 1 entities for 3 files to upload
  $ hg -R clone2 outgoing --large -T "{rev}:{node|short}\n"
  comparing with $TESTTMP/issue3651/src (glob)
  searching for changes
  1:1acbe71ce432
  2:6095d0695d70
  largefiles to upload (1 entities):
  b
  b1
  b2
  
  $ hg -R clone2 cat -r 1 clone2/.hglf/b
  89e6c98d92887913cadf06b2adb97f26cde4849b
  $ hg -R clone2 outgoing --large -T "{rev}:{node|short}\n" --debug --config progress.debug=true
  comparing with $TESTTMP/issue3651/src (glob)
  query 1; heads
  searching for changes
  all remote heads known locally
  1:1acbe71ce432
  2:6095d0695d70
  finding outgoing largefiles: 0/2 revision (0.00%)
  finding outgoing largefiles: 1/2 revision (50.00%)
  largefiles to upload (1 entities):
  b
      89e6c98d92887913cadf06b2adb97f26cde4849b
  b1
      89e6c98d92887913cadf06b2adb97f26cde4849b
  b2
      89e6c98d92887913cadf06b2adb97f26cde4849b
  

  $ echo bbb > clone2/b
  $ hg -R clone2 commit -m '#3: add new largefile entity as existing file'
  Invoking status precommit hook
  M b
  $ echo bbbb > clone2/b
  $ hg -R clone2 commit -m '#4: add new largefile entity as existing file'
  Invoking status precommit hook
  M b
  $ cp clone2/b1 clone2/b
  $ hg -R clone2 commit -m '#5: refer existing largefile entity again'
  Invoking status precommit hook
  M b
  $ hg -R clone2 summary --large
  parent: 5:036794ea641c tip
   #5: refer existing largefile entity again
  branch: default
  commit: (clean)
  update: (current)
  phases: 6 draft
  largefiles: 3 entities for 3 files to upload
  $ hg -R clone2 outgoing --large -T "{rev}:{node|short}\n"
  comparing with $TESTTMP/issue3651/src (glob)
  searching for changes
  1:1acbe71ce432
  2:6095d0695d70
  3:7983dce246cc
  4:233f12ada4ae
  5:036794ea641c
  largefiles to upload (3 entities):
  b
  b1
  b2
  
  $ hg -R clone2 cat -r 3 clone2/.hglf/b
  c801c9cfe94400963fcb683246217d5db77f9a9a
  $ hg -R clone2 cat -r 4 clone2/.hglf/b
  13f9ed0898e315bf59dc2973fec52037b6f441a2
  $ hg -R clone2 outgoing --large -T "{rev}:{node|short}\n" --debug --config progress.debug=true
  comparing with $TESTTMP/issue3651/src (glob)
  query 1; heads
  searching for changes
  all remote heads known locally
  1:1acbe71ce432
  2:6095d0695d70
  3:7983dce246cc
  4:233f12ada4ae
  5:036794ea641c
  finding outgoing largefiles: 0/5 revision (0.00%)
  finding outgoing largefiles: 1/5 revision (20.00%)
  finding outgoing largefiles: 2/5 revision (40.00%)
  finding outgoing largefiles: 3/5 revision (60.00%)
  finding outgoing largefiles: 4/5 revision (80.00%)
  largefiles to upload (3 entities):
  b
      13f9ed0898e315bf59dc2973fec52037b6f441a2
      89e6c98d92887913cadf06b2adb97f26cde4849b
      c801c9cfe94400963fcb683246217d5db77f9a9a
  b1
      89e6c98d92887913cadf06b2adb97f26cde4849b
  b2
      89e6c98d92887913cadf06b2adb97f26cde4849b
  

Pushing revision #1 causes uploading entity 89e6c98d9288, which is
shared also by largefiles b1, b2 in revision #2 and b in revision #5.

Then, entity 89e6c98d9288 is not treated as "outgoing entity" at "hg
summary" and "hg outgoing", even though files in outgoing revision #2
and #5 refer it.

  $ hg -R clone2 push -r 1 -q
  $ hg -R clone2 summary --large
  parent: 5:036794ea641c tip
   #5: refer existing largefile entity again
  branch: default
  commit: (clean)
  update: (current)
  phases: 6 draft
  largefiles: 2 entities for 1 files to upload
  $ hg -R clone2 outgoing --large -T "{rev}:{node|short}\n"
  comparing with $TESTTMP/issue3651/src (glob)
  searching for changes
  2:6095d0695d70
  3:7983dce246cc
  4:233f12ada4ae
  5:036794ea641c
  largefiles to upload (2 entities):
  b
  
  $ hg -R clone2 outgoing --large -T "{rev}:{node|short}\n" --debug --config progress.debug=true
  comparing with $TESTTMP/issue3651/src (glob)
  query 1; heads
  searching for changes
  all remote heads known locally
  2:6095d0695d70
  3:7983dce246cc
  4:233f12ada4ae
  5:036794ea641c
  finding outgoing largefiles: 0/4 revision (0.00%)
  finding outgoing largefiles: 1/4 revision (25.00%)
  finding outgoing largefiles: 2/4 revision (50.00%)
  finding outgoing largefiles: 3/4 revision (75.00%)
  largefiles to upload (2 entities):
  b
      13f9ed0898e315bf59dc2973fec52037b6f441a2
      c801c9cfe94400963fcb683246217d5db77f9a9a
  

  $ cd ..

merge action 'd' for 'local renamed directory to d2/g' which has no filename
==================================================================================

  $ hg init merge-action
  $ cd merge-action
  $ touch l
  $ hg add --large l
  $ mkdir d1
  $ touch d1/f
  $ hg ci -Aqm0
  Invoking status precommit hook
  A d1/f
  A l
  $ echo > d1/f
  $ touch d1/g
  $ hg ci -Aqm1
  Invoking status precommit hook
  M d1/f
  A d1/g
  $ hg up -qr0
  $ hg mv d1 d2
  moving d1/f to d2/f (glob)
  $ hg ci -qm2
  Invoking status precommit hook
  A d2/f
  R d1/f
  $ hg merge
  merging d2/f and d1/f to d2/f
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cd ..


Merge conflicts:
=====================

  $ hg init merge
  $ cd merge
  $ echo 0 > f-different
  $ echo 0 > f-same
  $ echo 0 > f-unchanged-1
  $ echo 0 > f-unchanged-2
  $ hg add --large *
  $ hg ci -m0
  Invoking status precommit hook
  A f-different
  A f-same
  A f-unchanged-1
  A f-unchanged-2
  $ echo tmp1 > f-unchanged-1
  $ echo tmp1 > f-unchanged-2
  $ echo tmp1 > f-same
  $ hg ci -m1
  Invoking status precommit hook
  M f-same
  M f-unchanged-1
  M f-unchanged-2
  $ echo 2 > f-different
  $ echo 0 > f-unchanged-1
  $ echo 1 > f-unchanged-2
  $ echo 1 > f-same
  $ hg ci -m2
  Invoking status precommit hook
  M f-different
  M f-same
  M f-unchanged-1
  M f-unchanged-2
  $ hg up -qr0
  $ echo tmp2 > f-unchanged-1
  $ echo tmp2 > f-unchanged-2
  $ echo tmp2 > f-same
  $ hg ci -m3
  Invoking status precommit hook
  M f-same
  M f-unchanged-1
  M f-unchanged-2
  created new head
  $ echo 1 > f-different
  $ echo 1 > f-unchanged-1
  $ echo 0 > f-unchanged-2
  $ echo 1 > f-same
  $ hg ci -m4
  Invoking status precommit hook
  M f-different
  M f-same
  M f-unchanged-1
  M f-unchanged-2
  $ hg merge
  largefile f-different has a merge conflict
  ancestor was 09d2af8dd22201dd8d48e5dcfcaed281ff9422c7
  keep (l)ocal e5fa44f2b31c1fb553b6021e7360d07d5d91ff5e or
  take (o)ther 7448d8798a4380162d4b56f9b452e2f6f9e24e7a? l
  getting changed largefiles
  1 largefiles updated, 0 removed
  0 files updated, 4 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f-different
  1
  $ cat f-same
  1
  $ cat f-unchanged-1
  1
  $ cat f-unchanged-2
  1
  $ cd ..

Test largefile insulation (do not enabled a side effect
========================================================

Check whether "largefiles" feature is supported only in repositories
enabling largefiles extension.

  $ mkdir individualenabling
  $ cd individualenabling

  $ hg init enabledlocally
  $ echo large > enabledlocally/large
  $ hg -R enabledlocally add --large enabledlocally/large
  $ hg -R enabledlocally commit -m '#0'
  Invoking status precommit hook
  A large

  $ hg init notenabledlocally
  $ echo large > notenabledlocally/large
  $ hg -R notenabledlocally add --large notenabledlocally/large
  $ hg -R notenabledlocally commit -m '#0'
  Invoking status precommit hook
  A large

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > # disable globally
  > largefiles=!
  > EOF
  $ cat >> enabledlocally/.hg/hgrc <<EOF
  > [extensions]
  > # enable locally
  > largefiles=
  > EOF
  $ hg -R enabledlocally root
  $TESTTMP/individualenabling/enabledlocally (glob)
  $ hg -R notenabledlocally root
  abort: repository requires features unknown to this Mercurial: largefiles!
  (see http://mercurial.selenic.com/wiki/MissingRequirement for more information)
  [255]

  $ hg init push-dst
  $ hg -R enabledlocally push push-dst
  pushing to push-dst
  abort: required features are not supported in the destination: largefiles
  [255]

  $ hg init pull-src
  $ hg -R pull-src pull enabledlocally
  pulling from enabledlocally
  abort: required features are not supported in the destination: largefiles
  [255]

  $ hg clone enabledlocally clone-dst
  abort: repository requires features unknown to this Mercurial: largefiles!
  (see http://mercurial.selenic.com/wiki/MissingRequirement for more information)
  [255]
  $ test -d clone-dst
  [1]
  $ hg clone --pull enabledlocally clone-pull-dst
  abort: required features are not supported in the destination: largefiles
  [255]
  $ test -d clone-pull-dst
  [1]

#if serve

Test largefiles specific peer setup, when largefiles is enabled
locally (issue4109)

  $ hg showconfig extensions | grep largefiles
  extensions.largefiles=!
  $ mkdir -p $TESTTMP/individualenabling/usercache

  $ hg serve -R enabledlocally -d -p $HGPORT --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS

  $ hg init pull-dst
  $ cat > pull-dst/.hg/hgrc <<EOF
  > [extensions]
  > # enable locally
  > largefiles=
  > [largefiles]
  > # ignore system cache to force largefiles specific wire proto access
  > usercache=$TESTTMP/individualenabling/usercache
  > EOF
  $ hg -R pull-dst -q pull -u http://localhost:$HGPORT

  $ killdaemons.py
#endif

Test overridden functions work correctly even for repos disabling
largefiles (issue4547)

  $ hg showconfig extensions | grep largefiles
  extensions.largefiles=!

(test updating implied by clone)

  $ hg init enabled-but-no-largefiles
  $ echo normal1 > enabled-but-no-largefiles/normal1
  $ hg -R enabled-but-no-largefiles add enabled-but-no-largefiles/normal1
  $ hg -R enabled-but-no-largefiles commit -m '#0@enabled-but-no-largefiles'
  Invoking status precommit hook
  A normal1
  $ cat >> enabled-but-no-largefiles/.hg/hgrc <<EOF
  > [extensions]
  > # enable locally
  > largefiles=
  > EOF
  $ hg clone -q enabled-but-no-largefiles no-largefiles

(test rebasing implied by pull: precommit while rebasing unexpectedly
shows "normal3" as "?", because lfdirstate isn't yet written out at
that time)

  $ echo normal2 > enabled-but-no-largefiles/normal2
  $ hg -R enabled-but-no-largefiles add enabled-but-no-largefiles/normal2
  $ hg -R enabled-but-no-largefiles commit -m '#1@enabled-but-no-largefiles'
  Invoking status precommit hook
  A normal2

  $ echo normal3 > no-largefiles/normal3
  $ hg -R no-largefiles add no-largefiles/normal3
  $ hg -R no-largefiles commit -m '#1@no-largefiles'
  Invoking status precommit hook
  A normal3

  $ hg -R no-largefiles -q pull --rebase
  Invoking status precommit hook
  M normal3

(test reverting)

  $ hg init subrepo-root
  $ cat >> subrepo-root/.hg/hgrc <<EOF
  > [extensions]
  > # enable locally
  > largefiles=
  > EOF
  $ echo large > subrepo-root/large
  $ hg -R subrepo-root add --large subrepo-root/large
  $ hg clone -q no-largefiles subrepo-root/no-largefiles
  $ cat > subrepo-root/.hgsub <<EOF
  > no-largefiles = no-largefiles
  > EOF
  $ hg -R subrepo-root add subrepo-root/.hgsub
  $ hg -R subrepo-root commit -m '#0'
  Invoking status precommit hook
  A .hgsub
  A large
  ? .hgsubstate
  $ echo dirty >> subrepo-root/large
  $ echo dirty >> subrepo-root/no-largefiles/normal1
  $ hg -R subrepo-root status -S
  M large
  M no-largefiles/normal1
  $ hg -R subrepo-root revert --all
  reverting subrepo-root/.hglf/large (glob)
  reverting subrepo no-largefiles
  reverting subrepo-root/no-largefiles/normal1 (glob)

  $ cd ..


Test "pull --rebase" when rebase is enabled before largefiles (issue3861)
=========================================================================

  $ hg showconfig extensions | grep largefiles
  extensions.largefiles=!

  $ mkdir issue3861
  $ cd issue3861
  $ hg init src
  $ hg clone -q src dst
  $ echo a > src/a
  $ hg -R src commit -Aqm "#0"
  Invoking status precommit hook
  A a

  $ cat >> dst/.hg/hgrc <<EOF
  > [extensions]
  > largefiles=
  > EOF
  $ hg -R dst pull --rebase
  pulling from $TESTTMP/issue3861/src (glob)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  nothing to rebase - working directory parent is already an ancestor of destination bf5e395ced2c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ..
