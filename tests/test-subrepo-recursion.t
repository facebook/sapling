Create test repository:

  $ hg init repo
  $ cd repo
  $ echo x1 > x.txt

  $ hg init foo
  $ cd foo
  $ echo y1 > y.txt

  $ hg init bar
  $ cd bar
  $ echo z1 > z.txt

  $ cd ..
  $ echo 'bar = bar' > .hgsub

  $ cd ..
  $ echo 'foo = foo' > .hgsub

Add files --- .hgsub files must go first to trigger subrepos:

  $ hg add -S .hgsub
  $ hg add -S foo/.hgsub
  $ hg add -S foo/bar
  adding foo/bar/z.txt (glob)
  $ hg add -S
  adding x.txt
  adding foo/y.txt (glob)

Test recursive status without committing anything:

  $ hg status -S
  A .hgsub
  A foo/.hgsub
  A foo/bar/z.txt
  A foo/y.txt
  A x.txt

Test recursive diff without committing anything:

  $ hg diff --nodates -S foo
  diff -r 000000000000 foo/.hgsub
  --- /dev/null
  +++ b/foo/.hgsub
  @@ -0,0 +1,1 @@
  +bar = bar
  diff -r 000000000000 foo/y.txt
  --- /dev/null
  +++ b/foo/y.txt
  @@ -0,0 +1,1 @@
  +y1
  diff -r 000000000000 foo/bar/z.txt
  --- /dev/null
  +++ b/foo/bar/z.txt
  @@ -0,0 +1,1 @@
  +z1

Commits:

  $ hg commit -m fails
  abort: uncommitted changes in subrepository 'foo'
  (use --subrepos for recursive commit)
  [255]

The --subrepos flag overwrite the config setting:

  $ hg commit -m 0-0-0 --config ui.commitsubrepos=No --subrepos
  committing subrepository foo
  committing subrepository foo/bar (glob)

  $ cd foo
  $ echo y2 >> y.txt
  $ hg commit -m 0-1-0

  $ cd bar
  $ echo z2 >> z.txt
  $ hg commit -m 0-1-1

  $ cd ..
  $ hg commit -m 0-2-1

  $ cd ..
  $ hg commit -m 1-2-1

Change working directory:

  $ echo y3 >> foo/y.txt
  $ echo z3 >> foo/bar/z.txt
  $ hg status -S
  M foo/bar/z.txt
  M foo/y.txt
  $ hg diff --nodates -S
  diff -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,2 +1,3 @@
   y1
   y2
  +y3
  diff -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,2 +1,3 @@
   z1
   z2
  +z3

Status call crossing repository boundaries:

  $ hg status -S foo/bar/z.txt
  M foo/bar/z.txt
  $ hg status -S -I 'foo/?.txt'
  M foo/y.txt
  $ hg status -S -I '**/?.txt'
  M foo/bar/z.txt
  M foo/y.txt
  $ hg diff --nodates -S -I '**/?.txt'
  diff -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,2 +1,3 @@
   y1
   y2
  +y3
  diff -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,2 +1,3 @@
   z1
   z2
  +z3

Status from within a subdirectory:

  $ mkdir dir
  $ cd dir
  $ echo a1 > a.txt
  $ hg status -S
  M foo/bar/z.txt
  M foo/y.txt
  ? dir/a.txt
  $ hg diff --nodates -S
  diff -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,2 +1,3 @@
   y1
   y2
  +y3
  diff -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,2 +1,3 @@
   z1
   z2
  +z3

Status with relative path:

  $ hg status -S ..
  M ../foo/bar/z.txt
  M ../foo/y.txt
  ? a.txt

XXX: filtering lfilesrepo.status() in 3.3-rc causes these files to be listed as
added instead of modified.
  $ hg status -S .. --config extensions.largefiles=
  M ../foo/bar/z.txt
  M ../foo/y.txt
  ? a.txt

  $ hg diff --nodates -S ..
  diff -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,2 +1,3 @@
   y1
   y2
  +y3
  diff -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,2 +1,3 @@
   z1
   z2
  +z3
  $ cd ..

Cleanup and final commit:

  $ rm -r dir
  $ hg commit --subrepos -m 2-3-2
  committing subrepository foo
  committing subrepository foo/bar (glob)

Test explicit path commands within subrepos: add/forget
  $ echo z1 > foo/bar/z2.txt
  $ hg status -S
  ? foo/bar/z2.txt
  $ hg add foo/bar/z2.txt
  $ hg status -S
  A foo/bar/z2.txt
  $ hg forget foo/bar/z2.txt
  $ hg status -S
  ? foo/bar/z2.txt
  $ hg forget foo/bar/z2.txt
  not removing foo/bar/z2.txt: file is already untracked (glob)
  [1]
  $ hg status -S
  ? foo/bar/z2.txt
  $ rm foo/bar/z2.txt

Log with the relationships between repo and its subrepo:

  $ hg log --template '{rev}:{node|short} {desc}\n'
  2:1326fa26d0c0 2-3-2
  1:4b3c9ff4f66b 1-2-1
  0:23376cbba0d8 0-0-0

  $ hg -R foo log --template '{rev}:{node|short} {desc}\n'
  3:65903cebad86 2-3-2
  2:d254738c5f5e 0-2-1
  1:8629ce7dcc39 0-1-0
  0:af048e97ade2 0-0-0

  $ hg -R foo/bar log --template '{rev}:{node|short} {desc}\n'
  2:31ecbdafd357 2-3-2
  1:9647f22de499 0-1-1
  0:4904098473f9 0-0-0

Status between revisions:

  $ hg status -S
  $ hg status -S --rev 0:1
  M .hgsubstate
  M foo/.hgsubstate
  M foo/bar/z.txt
  M foo/y.txt
  $ hg diff --nodates -S -I '**/?.txt' --rev 0:1
  diff -r af048e97ade2 -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,1 +1,2 @@
   y1
  +y2
  diff -r 4904098473f9 -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,1 +1,2 @@
   z1
  +z2

Enable progress extension for archive tests:

  $ cp $HGRCPATH $HGRCPATH.no-progress
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > progress =
  > [progress]
  > assume-tty = 1
  > delay = 0
  > # set changedelay really large so we don't see nested topics
  > changedelay = 30000
  > format = topic bar number
  > refresh = 0
  > width = 60
  > EOF

Test archiving to a directory tree (the doubled lines in the output
only show up in the test output, not in real usage):

  $ hg archive --subrepos ../archive
  \r (no-eol) (esc)
  archiving [                                           ] 0/3\r (no-eol) (esc)
  archiving [                                           ] 0/3\r (no-eol) (esc)
  archiving [=============>                             ] 1/3\r (no-eol) (esc)
  archiving [=============>                             ] 1/3\r (no-eol) (esc)
  archiving [===========================>               ] 2/3\r (no-eol) (esc)
  archiving [===========================>               ] 2/3\r (no-eol) (esc)
  archiving [==========================================>] 3/3\r (no-eol) (esc)
  archiving [==========================================>] 3/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  archiving (foo) [                                     ] 0/3\r (no-eol) (esc)
  archiving (foo) [                                     ] 0/3\r (no-eol) (esc)
  archiving (foo) [===========>                         ] 1/3\r (no-eol) (esc)
  archiving (foo) [===========>                         ] 1/3\r (no-eol) (esc)
  archiving (foo) [=======================>             ] 2/3\r (no-eol) (esc)
  archiving (foo) [=======================>             ] 2/3\r (no-eol) (esc)
  archiving (foo) [====================================>] 3/3\r (no-eol) (esc)
  archiving (foo) [====================================>] 3/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  archiving (foo/bar) [                                 ] 0/1\r (no-eol) (glob) (esc)
  archiving (foo/bar) [                                 ] 0/1\r (no-eol) (glob) (esc)
  archiving (foo/bar) [================================>] 1/1\r (no-eol) (glob) (esc)
  archiving (foo/bar) [================================>] 1/1\r (no-eol) (glob) (esc)
                                                              \r (no-eol) (esc)
  $ find ../archive | sort
  ../archive
  ../archive/.hg_archival.txt
  ../archive/.hgsub
  ../archive/.hgsubstate
  ../archive/foo
  ../archive/foo/.hgsub
  ../archive/foo/.hgsubstate
  ../archive/foo/bar
  ../archive/foo/bar/z.txt
  ../archive/foo/y.txt
  ../archive/x.txt

Test archiving to zip file (unzip output is unstable):

  $ hg archive --subrepos --prefix '.' ../archive.zip
  \r (no-eol) (esc)
  archiving [                                           ] 0/3\r (no-eol) (esc)
  archiving [                                           ] 0/3\r (no-eol) (esc)
  archiving [=============>                             ] 1/3\r (no-eol) (esc)
  archiving [=============>                             ] 1/3\r (no-eol) (esc)
  archiving [===========================>               ] 2/3\r (no-eol) (esc)
  archiving [===========================>               ] 2/3\r (no-eol) (esc)
  archiving [==========================================>] 3/3\r (no-eol) (esc)
  archiving [==========================================>] 3/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  archiving (foo) [                                     ] 0/3\r (no-eol) (esc)
  archiving (foo) [                                     ] 0/3\r (no-eol) (esc)
  archiving (foo) [===========>                         ] 1/3\r (no-eol) (esc)
  archiving (foo) [===========>                         ] 1/3\r (no-eol) (esc)
  archiving (foo) [=======================>             ] 2/3\r (no-eol) (esc)
  archiving (foo) [=======================>             ] 2/3\r (no-eol) (esc)
  archiving (foo) [====================================>] 3/3\r (no-eol) (esc)
  archiving (foo) [====================================>] 3/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  archiving (foo/bar) [                                 ] 0/1\r (no-eol) (glob) (esc)
  archiving (foo/bar) [                                 ] 0/1\r (no-eol) (glob) (esc)
  archiving (foo/bar) [================================>] 1/1\r (no-eol) (glob) (esc)
  archiving (foo/bar) [================================>] 1/1\r (no-eol) (glob) (esc)
                                                              \r (no-eol) (esc)
  $ unzip -l ../archive.zip
  Archive:  ../archive.zip
    Length      Date    Time    Name
  ---------  ---------- -----   ----
        172  01-01-1980 00:00   .hg_archival.txt
         10  01-01-1980 00:00   .hgsub
         45  01-01-1980 00:00   .hgsubstate
          3  01-01-1980 00:00   x.txt
         10  01-01-1980 00:00   foo/.hgsub
         45  01-01-1980 00:00   foo/.hgsubstate
          9  01-01-1980 00:00   foo/y.txt
          9  01-01-1980 00:00   foo/bar/z.txt
  ---------                     -------
        303                     8 files

Test archiving a revision that references a subrepo that is not yet
cloned:

#if hardlink
  $ hg clone -U . ../empty
  \r (no-eol) (esc)
  linking [ <=>                                           ] 1\r (no-eol) (esc)
  linking [  <=>                                          ] 2\r (no-eol) (esc)
  linking [   <=>                                         ] 3\r (no-eol) (esc)
  linking [    <=>                                        ] 4\r (no-eol) (esc)
  linking [     <=>                                       ] 5\r (no-eol) (esc)
  linking [      <=>                                      ] 6\r (no-eol) (esc)
  linking [       <=>                                     ] 7\r (no-eol) (esc)
  linking [        <=>                                    ] 8\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
#else
  $ hg clone -U . ../empty
  \r (no-eol) (esc)
  linking [ <=>                                           ] 1 (no-eol)
#endif

  $ cd ../empty
#if hardlink
  $ hg archive --subrepos -r tip --prefix './' ../archive.tar.gz
  \r (no-eol) (esc)
  archiving [                                           ] 0/3\r (no-eol) (esc)
  archiving [                                           ] 0/3\r (no-eol) (esc)
  archiving [=============>                             ] 1/3\r (no-eol) (esc)
  archiving [=============>                             ] 1/3\r (no-eol) (esc)
  archiving [===========================>               ] 2/3\r (no-eol) (esc)
  archiving [===========================>               ] 2/3\r (no-eol) (esc)
  archiving [==========================================>] 3/3\r (no-eol) (esc)
  archiving [==========================================>] 3/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  linking [ <=>                                           ] 1\r (no-eol) (esc)
  linking [  <=>                                          ] 2\r (no-eol) (esc)
  linking [   <=>                                         ] 3\r (no-eol) (esc)
  linking [    <=>                                        ] 4\r (no-eol) (esc)
  linking [     <=>                                       ] 5\r (no-eol) (esc)
  linking [      <=>                                      ] 6\r (no-eol) (esc)
  linking [       <=>                                     ] 7\r (no-eol) (esc)
  linking [        <=>                                    ] 8\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  archiving (foo) [                                     ] 0/3\r (no-eol) (esc)
  archiving (foo) [                                     ] 0/3\r (no-eol) (esc)
  archiving (foo) [===========>                         ] 1/3\r (no-eol) (esc)
  archiving (foo) [===========>                         ] 1/3\r (no-eol) (esc)
  archiving (foo) [=======================>             ] 2/3\r (no-eol) (esc)
  archiving (foo) [=======================>             ] 2/3\r (no-eol) (esc)
  archiving (foo) [====================================>] 3/3\r (no-eol) (esc)
  archiving (foo) [====================================>] 3/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  linking [ <=>                                           ] 1\r (no-eol) (esc)
  linking [  <=>                                          ] 2\r (no-eol) (esc)
  linking [   <=>                                         ] 3\r (no-eol) (esc)
  linking [    <=>                                        ] 4\r (no-eol) (esc)
  linking [     <=>                                       ] 5\r (no-eol) (esc)
  linking [      <=>                                      ] 6\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  archiving (foo/bar) [                                 ] 0/1\r (no-eol) (glob) (esc)
  archiving (foo/bar) [                                 ] 0/1\r (no-eol) (glob) (esc)
  archiving (foo/bar) [================================>] 1/1\r (no-eol) (glob) (esc)
  archiving (foo/bar) [================================>] 1/1\r (no-eol) (glob) (esc)
                                                              \r (no-eol) (esc)
  cloning subrepo foo from $TESTTMP/repo/foo
  cloning subrepo foo/bar from $TESTTMP/repo/foo/bar (glob)
#else
Note there's a slight output glitch on non-hardlink systems: the last
"linking" progress topic never gets closed, leading to slight output corruption on that platform.
  $ hg archive --subrepos -r tip --prefix './' ../archive.tar.gz
  \r (no-eol) (esc)
  archiving [                                           ] 0/3\r (no-eol) (esc)
  archiving [                                           ] 0/3\r (no-eol) (esc)
  archiving [=============>                             ] 1/3\r (no-eol) (esc)
  archiving [=============>                             ] 1/3\r (no-eol) (esc)
  archiving [===========================>               ] 2/3\r (no-eol) (esc)
  archiving [===========================>               ] 2/3\r (no-eol) (esc)
  archiving [==========================================>] 3/3\r (no-eol) (esc)
  archiving [==========================================>] 3/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  linking [ <=>                                           ] 1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  linking [  <=>                                          ] 1cloning subrepo foo from $TESTTMP/repo/foo
  cloning subrepo foo/bar from $TESTTMP/repo/foo/bar (glob)
#endif

Archive + subrepos uses '/' for all component separators

  $ tar -tzf ../archive.tar.gz | sort
  .hg_archival.txt
  .hgsub
  .hgsubstate
  foo/.hgsub
  foo/.hgsubstate
  foo/bar/z.txt
  foo/y.txt
  x.txt

The newly cloned subrepos contain no working copy:

  $ hg -R foo summary
  parent: -1:000000000000  (no revision checked out)
  branch: default
  commit: (clean)
  update: 4 new changesets (update)

Disable progress extension and cleanup:

  $ mv $HGRCPATH.no-progress $HGRCPATH

Test archiving when there is a directory in the way for a subrepo
created by archive:

  $ hg clone -U . ../almost-empty
  $ cd ../almost-empty
  $ mkdir foo
  $ echo f > foo/f
  $ hg archive --subrepos -r tip archive
  cloning subrepo foo from $TESTTMP/empty/foo
  abort: destination '$TESTTMP/almost-empty/foo' is not empty (in subrepo foo) (glob)
  [255]

Clone and test outgoing:

  $ cd ..
  $ hg clone repo repo2
  updating to branch default
  cloning subrepo foo from $TESTTMP/repo/foo
  cloning subrepo foo/bar from $TESTTMP/repo/foo/bar (glob)
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2
  $ hg outgoing -S
  comparing with $TESTTMP/repo (glob)
  searching for changes
  no changes found
  comparing with $TESTTMP/repo/foo
  searching for changes
  no changes found
  comparing with $TESTTMP/repo/foo/bar
  searching for changes
  no changes found
  [1]

Make nested change:

  $ echo y4 >> foo/y.txt
  $ hg diff --nodates -S
  diff -r 65903cebad86 foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,3 +1,4 @@
   y1
   y2
   y3
  +y4
  $ hg commit --subrepos -m 3-4-2
  committing subrepository foo
  $ hg outgoing -S
  comparing with $TESTTMP/repo (glob)
  searching for changes
  changeset:   3:2655b8ecc4ee
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3-4-2
  
  comparing with $TESTTMP/repo/foo
  searching for changes
  changeset:   4:e96193d6cb36
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3-4-2
  
  comparing with $TESTTMP/repo/foo/bar
  searching for changes
  no changes found


Switch to original repo and setup default path:

  $ cd ../repo
  $ echo '[paths]' >> .hg/hgrc
  $ echo 'default = ../repo2' >> .hg/hgrc

Test incoming:

  $ hg incoming -S
  comparing with $TESTTMP/repo2 (glob)
  searching for changes
  changeset:   3:2655b8ecc4ee
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3-4-2
  
  comparing with $TESTTMP/repo2/foo
  searching for changes
  changeset:   4:e96193d6cb36
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3-4-2
  
  comparing with $TESTTMP/repo2/foo/bar
  searching for changes
  no changes found

  $ hg incoming -S --bundle incoming.hg
  abort: cannot combine --bundle and --subrepos
  [255]

Test missing subrepo:

  $ rm -r foo
  $ hg status -S
  warning: error "unknown revision '65903cebad86f1a84bd4f1134f62fa7dcb7a1c98'" in subrepository "foo"

Issue2619: IndexError: list index out of range on hg add with subrepos
The subrepo must sorts after the explicit filename.

  $ cd ..
  $ hg init test
  $ cd test
  $ hg init x
  $ echo abc > abc.txt
  $ hg ci -Am "abc"
  adding abc.txt
  $ echo "x = x" >> .hgsub
  $ hg add .hgsub
  $ touch a x/a
  $ hg add a x/a

  $ hg ci -Sm "added x"
  committing subrepository x
  $ echo abc > x/a
  $ hg revert --rev '.^' "set:subrepo('glob:x*')"
  abort: subrepository 'x' does not exist in 25ac2c9b3180!
  [255]

  $ cd ..
