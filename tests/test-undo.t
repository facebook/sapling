  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > undo = $TESTDIR/../hgext3rd/undo.py
  > inhibit=$TESTDIR/../hgext3rd/inhibit.py
  > [undo]
  > _duringundologlock=1
  > [experimental]
  > evolution=createmarkers
  > [ui]
  > interactive = true
  > EOF

Build up a repo

  $ hg init repo
  $ cd repo

Test data store

  $ hg book master
  $ touch a1 && hg add a1 && hg ci -ma1
  $ touch a2 && hg add a2 && hg ci -ma2
  $ hg book feature1
  $ touch b && hg add b && hg ci -mb
  $ hg up -q master
  $ touch c1 && hg add c1 && hg ci -mc1
  created new head
  $ touch c2 && hg add c2 && hg ci -mc2
  $ hg book feature2
  $ touch d && hg add d && hg ci -md
  $ hg debugindex .hg/undolog/command.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       0      0       1 b80de5d13875 000000000000 000000000000
       1         0      12      1       1 440cdcef588f 000000000000 000000000000
       2        12       8      2       1 86fddc37572c 000000000000 000000000000
       3        20       8      3       1 388d40a434df 000000000000 000000000000
       4        28      14      4       1 1cafbfad488a 000000000000 000000000000
       5        42       7      5       1 8879b3bd818b 000000000000 000000000000
       6        49      13      6       1 b0f66da09921 000000000000 000000000000
       7        62       8      7       1 004b7198dafe 000000000000 000000000000
       8        70       8      8       1 60920018c706 000000000000 000000000000
       9        78      14      9       1 c3e212568400 000000000000 000000000000
      10        92       7     10       1 9d609b5b001c 000000000000 000000000000
  $ hg debugdata .hg/undolog/command.i 0
  $ hg debugdata .hg/undolog/command.i 1
  book\x00master (no-eol) (esc)
  $ hg debugdata .hg/undolog/command.i 2
  ci\x00-ma1 (no-eol) (esc)
  $ hg debugdata .hg/undolog/command.i 3
  ci\x00-ma2 (no-eol) (esc)
  $ hg debugdata .hg/undolog/bookmarks.i 0
  $ hg debugdata .hg/undolog/bookmarks.i 1
  master 0000000000000000000000000000000000000000 (no-eol)
  $ hg debugdata .hg/undolog/bookmarks.i 2
  master df4fd610a3d6ca792281e7576587fa18f940d37a (no-eol)
  $ hg debugdata .hg/undolog/workingparent.i 0
  0000000000000000000000000000000000000000 (no-eol)
  $ hg debugdata .hg/undolog/workingparent.i 1
  df4fd610a3d6ca792281e7576587fa18f940d37a (no-eol)
  $ hg debugdata .hg/undolog/draftheads.i 1
  df4fd610a3d6ca792281e7576587fa18f940d37a (no-eol)
  $ hg debugdata .hg/undolog/draftheads.i 2
  b68836a6e2cac33ba33a20249b85a486eec78186 (no-eol)
  $ hg debugdata .hg/undolog/index.i 1
  bookmarks 8153d44860d076e9c328951c8f36cf8daebe695a
  command 440cdcef588f9a594c5530a5b6dede39a96d930d
  date * (glob)
  draftheads b80de5d138758541c5f05265ad144ab9fa86d1db
  workingparent fcb754f6a51eaf982f66d0637b39f3d2e6b520d5 (no-eol)
  $ touch a3 && hg add a3
  $ hg commit --amend
  $ hg debugdata .hg/undolog/command.i 11
  commit\x00--amend (no-eol) (esc)

Test debugundohistory
  $ hg debugundohistory -l
  0: commit --amend
  1: ci -md
  2: book feature2
  3: ci -mc2
  4: ci -mc1
  $ hg update master
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (activating bookmark master)
  $ echo "test" >> a1
  $ hg commit -m "words"
  created new head
  $ hg debugundohistory -l
  0: commit -m words
  1: update master
  2: commit --amend
  3: ci -md
  4: book feature2
  $ hg debugundohistory -n 0
  command:
  	commit -m words
  bookmarks:
  	feature1 49cdb4091aca3c09f402ff001cd20cf086873683
  	feature2 296fda51a303650465d07a1cd054075cbe6d3cbd
  	master 0a3dd3e15e65b90836f492112d816f3ee073d897
  date:
  	* (glob)
  draftheads:
  	ADDED:
  		0a3dd3e15e65b90836f492112d816f3ee073d897
  	REMOVED:
  	
  workingparent:
  	0a3dd3e15e65b90836f492112d816f3ee073d897

Test gap in data (extension dis and enabled)
  $ hg debugundohistory -l
  0: commit -m words
  1: update master
  2: commit --amend
  3: ci -md
  4: book feature2
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > undo =!
  > EOF
  $ touch cmiss && hg add cmiss && hg ci -mcmiss
  $ cat >>$HGRCPATH <<EOF
  > [extensions]
  > undo = $TESTDIR/../hgext3rd/undo.py
  > EOF
  $ touch a5 && hg add a5 && hg ci -ma5
  $ hg debugundohistory -l
  0: ci -ma5
  1:  -- gap in log -- 
  2: commit -m words
  3: update master
  4: commit --amend
  $ hg debugundohistory 1
  command:
  	unkown command(s) run, gap in log
  bookmarks:
  	feature1 49cdb4091aca3c09f402ff001cd20cf086873683
  	feature2 296fda51a303650465d07a1cd054075cbe6d3cbd
  	master 1dafc0b436123cab96f82a8e9e8d1d42c0301aaa
  date:
  	* (glob)
  draftheads:
  	ADDED:
  		1dafc0b436123cab96f82a8e9e8d1d42c0301aaa
  	REMOVED:
  		0a3dd3e15e65b90836f492112d816f3ee073d897
  workingparent:
  	1dafc0b436123cab96f82a8e9e8d1d42c0301aaa

Index out of bound error
  $ hg debugundohistory -n 50
  abort: index out of bounds
  [255]

Revset tests
  $ hg log -G -r 'draft()' --hidden > /dev/null
  $ hg debugundohistory -n 0
  command:
  	ci -ma5
  bookmarks:
  	feature1 49cdb4091aca3c09f402ff001cd20cf086873683
  	feature2 296fda51a303650465d07a1cd054075cbe6d3cbd
  	master aa430c8afedf9b2ec3f0655d39eef6b6b0a2ddb6
  date:
  	* (glob)
  draftheads:
  	ADDED:
  		aa430c8afedf9b2ec3f0655d39eef6b6b0a2ddb6
  	REMOVED:
  		1dafc0b436123cab96f82a8e9e8d1d42c0301aaa
  workingparent:
  	aa430c8afedf9b2ec3f0655d39eef6b6b0a2ddb6

Test 'olddraft([NUM])' revset
  $ hg log -G -r 'olddraft(0) - olddraft(1)' --hidden -T compact
  @  10[tip][master]   aa430c8afedf   1970-01-01 00:00 +0000   test
  |    a5
  ~

Test undolog lock
  $ hg log --config hooks.duringundologlock="sleep 1" > /dev/null &
  $ sleep 0.1
  $ hg st --time
  time: real [1-9]*\..* (re)

hg undo command tests
  $ hg undo
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark master)
  $ hg log -G -T compact -l2
  @  9[tip][master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  |
  o  8:4   0a3dd3e15e65   1970-01-01 00:00 +0000   test
  |    words
  ~
  $ hg update 0a3dd3e15e65
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg undo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T compact -l1
  @  9[tip][master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ touch c11 && hg add c11
  $ hg commit --amend
  $ hg log -G -T compact -l1
  @  12[tip][master]:8   2dca609174c2   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -G -T compact -l4
  @  9[tip][master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  |
  o  8:4   0a3dd3e15e65   1970-01-01 00:00 +0000   test
  |    words
  |
  | o  7[feature2]:4   296fda51a303   1970-01-01 00:00 +0000   test
  |/     d
  |
  o  4   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  ~
  $ hg graft 296fda51a303
  grafting 7:296fda51a303 "d" (feature2)
  $ hg log -G -T compact -l2
  @  13[tip]:9   f007a7cf4c3d   1970-01-01 00:00 +0000   test
  |    d
  |
  o  9[master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -G -T compact -l1
  @  9[tip][master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg book test
  $ hg undo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test)
  $ hg bookmarks
     feature1                  2:49cdb4091aca
     feature2                  7:296fda51a303
     master                    9:1dafc0b43612

hg redo test
  $ hg redo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T compact -l1
  @  9[tip][master,test]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T compact -l1
  @  9[tip][master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo -n 5
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg redo -n 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T compact -l1
  @  9[tip][master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg redo -n 100
  abort: index out of bounds
  [255]

hg undo --absolute tests
  $ hg undo -a
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg redo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg undo -a
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg redo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg undo -n 5
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T compact -l1
  @  9[tip][master,test]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo -a
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T compact -l1
  @  9[tip][master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~

hg undo --force tests
  $ hg debugundohistory -l 18
  18: undo
  19: ci -ma5
  20:  -- gap in log -- 
  21: commit -m words
  22: update master
  $ hg undo -a -n 25
  abort: attempted risky undo across missing history
  [255]
  $ hg undo -a -n 25 -f
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg undo -a
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

hg undo --keep tests
  $ touch kfl1 && hg add kfl1
  $ hg st
  A kfl1
  $ hg commit --amend
  $ hg st
  $ hg undo --keep
  $ hg st
  A kfl1
  $ hg commit --amend

hg undo informative obsmarkers
check 1 to 1 undos have informative obsmarker
check 1 to many undos (generally a redo of split or divergence) do not connect
the changesets with obsmarkers as we do not differentiate between split and
divergence cases in undo.  The original split/divergence obsmarkers suffice for
checking split/divergence.
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > smartlog = $TESTDIR/../hgext3rd/smartlog.py
  > tweakdefaults = $TESTDIR/../hgext3rd/tweakdefaults.py
  > fbamend = $TESTDIR/../hgext3rd/fbamend/
  > EOF
  $ hg undo
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg sl --all --hidden -T "{node|short} {if(undosuccessors, label('sl.undo', '(Undone as {join(undosuccessors% \'{shortest(undosuccessor, 6)}\', ', ')})'))}"
  x  db3723da827c
  |
  | x  f007a7cf4c3d
  |/
  | x  128fe7e6098d
  |/
  | x  aa430c8afedf
  |/
  @  1dafc0b43612
  :
  : x  c9476255bc2a (Undone as 1dafc0)
  :/
  : x  2dca609174c2
  :/
  : o  296fda51a303
  :/
  : x  551c0e5b57c9
  : |
  : x  db92053d5c83
  :/
  : o  49cdb4091aca
  :/
  o  b68836a6e2ca
  |
  ~
  $ echo "a" >> newa && echo "b" >> newb && hg add newa newb && hg ci -m "newfiles"
  $ hg split --quiet << EOF
  > y
  > y
  > n
  > y
  > EOF
  diff --git a/newa b/newa
  new file mode 100644
  examine changes to 'newa'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +a
  record change 1/2 to 'newa'? [Ynesfdaq?] y
  
  diff --git a/newb b/newb
  new file mode 100644
  examine changes to 'newb'? [Ynesfdaq?] n
  
  Done splitting? [yN] y
  $ hg debugobsolete | tail -5
  db3723da827c373768d500ab4e3a9c59a78314a6 0 {1dafc0b436123cab96f82a8e9e8d1d42c0301aaa} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  c9476255bc2a68672c844021397838ff4eeefcda 1dafc0b436123cab96f82a8e9e8d1d42c0301aaa 0 (Thu Jan 01 00:00:04 1970 +0000) {'user': 'test'}
  1dafc0b436123cab96f82a8e9e8d1d42c0301aaa c9476255bc2a68672c844021397838ff4eeefcda 0 (Thu Jan 01 00:00:05 1970 +0000) {'user': 'test'}
  c9476255bc2a68672c844021397838ff4eeefcda 1dafc0b436123cab96f82a8e9e8d1d42c0301aaa 0 (Thu Jan 01 00:00:06 1970 +0000) {'operation': 'undo', 'user': 'test'}
  f86734247df6db66a810e549cc938a72cd5c6d1a d0fdb9510dbf78c1a7e62c3e6628ff1f978f87ea 75f63379f12bf02d40fe7444587ad67be9ae81b8 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'split', 'user': 'test'}
  $ hg undo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg redo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugobsolete | tail -5
  d0fdb9510dbf78c1a7e62c3e6628ff1f978f87ea f86734247df6db66a810e549cc938a72cd5c6d1a 0 (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'undo', 'user': 'test'}
  75f63379f12bf02d40fe7444587ad67be9ae81b8 0 {d0fdb9510dbf78c1a7e62c3e6628ff1f978f87ea} (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'undo', 'user': 'test'}
  f86734247df6db66a810e549cc938a72cd5c6d1a 0 {1dafc0b436123cab96f82a8e9e8d1d42c0301aaa} (Thu Jan 01 00:00:02 1970 +0000) {'operation': 'undo', 'user': 'test'}
  d0fdb9510dbf78c1a7e62c3e6628ff1f978f87ea d0fdb9510dbf78c1a7e62c3e6628ff1f978f87ea 0 (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'revive', 'user': 'test'}
  75f63379f12bf02d40fe7444587ad67be9ae81b8 75f63379f12bf02d40fe7444587ad67be9ae81b8 0 (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'revive', 'user': 'test'}
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > smartlog =!
  > tweakdefaults =!
  > fbamend =!
  > EOF

File corruption handling
  $ echo 111corruptedrevlog > .hg/undolog/index.i
#if chg
(note: chg has issues with the below test)
#else
  $ hg st --debug
  caught revlog error. undolog/index.i was probably corrupted
#endif
  $ hg debugundohistory -l
  0:  -- gap in log -- 
