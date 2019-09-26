  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > undo=
  > remotenames=
  > extralog=$TESTDIR/extralog.py
  > [experimental]
  > evolution=
  > narrow-heads=true
  > [visibility]
  > enabled=true
  > [mutation]
  > enabled=true
  > date=0 0
  > [ui]
  > interactive = true
  > EOF

Use chg if possible to speed up the test
  $ unset CHGDISABLE

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
  $ touch c2 && hg add c2 && hg ci -mc2
  $ hg book feature2
  $ touch d && hg add d && hg ci -md
  $ hg debugindex .hg/undolog/command.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       0      0       1 b80de5d13875 000000000000 000000000000
       1         0      22      1       1 bae41a9d0ae9 000000000000 000000000000
       2        22      15      2       1 6086ecd42962 000000000000 000000000000
       3        37      15      3       1 5cc3b22d9c65 000000000000 000000000000
       4        52      24      4       1 efaf8be657bb 000000000000 000000000000
       5        76      14      5       1 fe69d6bf5432 000000000000 000000000000
       6        90      20      6       1 720b2d7eb02b 000000000000 000000000000
       7       110      15      7       1 0595892db30f 000000000000 000000000000
       8       125      15      8       1 c8265ade52fa 000000000000 000000000000
       9       140      24      9       1 0d8f98a931b9 000000000000 000000000000
      10       164      14     10       1 fded10d93e92 000000000000 000000000000
  $ hg debugdata .hg/undolog/command.i 0
  $ hg debugdata .hg/undolog/command.i 1
  bookmarks\x00book\x00master (no-eol) (esc)
  $ hg debugdata .hg/undolog/command.i 2
  commit\x00ci\x00-ma1 (no-eol) (esc)
  $ hg debugdata .hg/undolog/command.i 3
  commit\x00ci\x00-ma2 (no-eol) (esc)
  $ hg debugdata .hg/undolog/bookmarks.i 0
  $ hg debugdata .hg/undolog/bookmarks.i 1
  master 0000000000000000000000000000000000000000 (no-eol)
  $ hg debugdata .hg/undolog/bookmarks.i 2
  master df4fd610a3d6ca792281e7576587fa18f940d37a (no-eol)
  $ hg debugdata .hg/undolog/workingparent.i 0
  0000000000000000000000000000000000000000 (no-eol)
  $ hg debugdata .hg/undolog/workingparent.i 1
  df4fd610a3d6ca792281e7576587fa18f940d37a (no-eol)
  $ hg debugdata .hg/undolog/visibleheads.i 1
  df4fd610a3d6ca792281e7576587fa18f940d37a (no-eol)
  $ hg debugdata .hg/undolog/visibleheads.i 2
  b68836a6e2cac33ba33a20249b85a486eec78186 (no-eol)
  $ hg debugdata .hg/undolog/index.i 1
  bookmarks 8153d44860d076e9c328951c8f36cf8daebe695a
  command bae41a9d0ae9614fc3aa843a0f5cbdf47bc98c43
  date * (glob)
  unfinished False
  visibleheads b80de5d138758541c5f05265ad144ab9fa86d1db
  workingparent fcb754f6a51eaf982f66d0637b39f3d2e6b520d5 (no-eol)
  $ touch a3 && hg add a3
  $ hg commit --amend
  $ hg debugdata .hg/undolog/command.i 11
  commit\x00commit\x00--amend (no-eol) (esc)

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
  visibleheads:
  	ADDED:
  		0a3dd3e15e65b90836f492112d816f3ee073d897
  	REMOVED:
  	
  unfinished:	False

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
  > undo=
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
  	unknown command(s) run, gap in log
  bookmarks:
  	feature1 49cdb4091aca3c09f402ff001cd20cf086873683
  	feature2 296fda51a303650465d07a1cd054075cbe6d3cbd
  	master 1dafc0b436123cab96f82a8e9e8d1d42c0301aaa
  date:
  	* (glob)
  visibleheads:
  	ADDED:
  		1dafc0b436123cab96f82a8e9e8d1d42c0301aaa
  	REMOVED:
  		0a3dd3e15e65b90836f492112d816f3ee073d897
  unfinished:	False

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
  visibleheads:
  	ADDED:
  		aa430c8afedf9b2ec3f0655d39eef6b6b0a2ddb6
  	REMOVED:
  		1dafc0b436123cab96f82a8e9e8d1d42c0301aaa
  unfinished:	False

Test 'olddraft([NUM])' revset
  $ hg log -G -r 'olddraft(0) - olddraft(1)' --hidden -T compact
  @  9[tip][master]   aa430c8afedf   1970-01-01 00:00 +0000   test
  |    a5
  ~
  $ hg log -G -r 'olddraft(1) and draft()' -T compact
  o  8   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  |
  o  7:4   0a3dd3e15e65   1970-01-01 00:00 +0000   test
  |    words
  |
  | o  6[feature2]:4   296fda51a303   1970-01-01 00:00 +0000   test
  |/     d
  |
  o  4   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  o  3:1   ec7553f7b382   1970-01-01 00:00 +0000   test
  |    c1
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |
  o  0   df4fd610a3d6   1970-01-01 00:00 +0000   test
       a1
  
  $ hg log -G -r 'olddraft(1) and public()' -T compact

hg status does not trigger undolog writing
  $ hg status --config extralog.events=undologlock

hg undo command tests
  $ hg undo --config extralog.events=undologlock
  undologlock: lock acquired
  undone to *, before ci -ma5 (glob)
  undologlock: lock acquired
  $ hg log -G -T compact -l2
  @  8[master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  |
  o  7:4   0a3dd3e15e65   1970-01-01 00:00 +0000   test
  |    words
  ~
  $ hg update 0a3dd3e15e65 --config extralog.events=undologlock
  undologlock: lock acquired
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark master)
  undologlock: lock acquired
  $ hg undo
  undone to *, before update 0a3dd3e15e65 --config extralog.events=undologlock (glob)
  $ hg log -G -T compact -l1
  @  8[master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ touch c11 && hg add c11
  $ hg commit --amend
  $ hg log -G -T compact -l1
  @  10[tip][master]:7   2dca609174c2   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo
  undone to *, before commit --amend (glob)
  hint[undo-uncommit-unamend]: undoing amends discards their changes.
  to restore the changes to the working copy, run 'hg revert -r 2dca609174c2 --all'
  in the future, you can use 'hg unamend' instead of 'hg undo' to keep changes
  hint[hint-ack]: use 'hg hint --ack undo-uncommit-unamend' to silence these hints
  $ hg log -G -T compact -l4
  @  8[master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  |
  o  7:4   0a3dd3e15e65   1970-01-01 00:00 +0000   test
  |    words
  |
  | o  6[feature2]:4   296fda51a303   1970-01-01 00:00 +0000   test
  |/     d
  |
  o  4   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  ~
  $ hg graft 296fda51a303
  grafting 6:296fda51a303 "d" (feature2)
  $ hg log -G -T compact -l2
  @  11[tip]:8   f007a7cf4c3d   1970-01-01 00:00 +0000   test
  |    d
  |
  o  8[master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo
  undone to *, before graft 296fda51a303 (glob)
  $ hg log -G -T compact -l1
  @  8[master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg book test
  $ hg undo
  undone to *, before book test (glob)
  $ hg bookmarks
     feature1                  2:49cdb4091aca
     feature2                  6:296fda51a303
     master                    8:1dafc0b43612

hg undo with negative step
  $ hg undo -n -1
  undone to *, before undo (glob)
  $ hg log -G -T compact -l1
  @  8[master,test]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo
  undone to *, before book test (glob)
  $ hg log -G -T compact -l1
  @  8[master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo -n 5
  undone to *, before undo (glob)
  $ hg undo -n -5
  undone to *, before book test (glob)
  $ hg log -G -T compact -l1
  @  8[master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo -n -100
  abort: cannot undo this far - undo extension was not enabled
  [255]

hg undo --absolute tests
  $ hg undo -a
  undone to *, before undo -n -5 (glob)
  $ hg undo -n -1
  undone to *, before undo -a (glob)
  $ hg undo -a
  undone to *, before undo -n -1 (glob)
  $ hg undo -n -1
  undone to *, before undo -a (glob)
  $ hg undo -n 5
  undone to *, before undo (glob)
  $ hg log -G -T compact -l1
  @  8[master,test]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~
  $ hg undo -a
  undone to *, before undo -n 5 (glob)
  $ hg log -G -T compact -l1
  @  8[master]   1dafc0b43612   1970-01-01 00:00 +0000   test
  |    cmiss
  ~

hg undo --force tests
  $ hg debugundohistory -l 18
  18: undo --config extralog.events=undologlock
  19: ci -ma5
  20:  -- gap in log -- 
  21: commit -m words
  22: update master
  $ hg undo -a -n 25
  abort: attempted risky undo across missing history
  [255]
  $ hg undo -a -n 25 -f
  undone to *, before ci -md (glob)
  $ hg undo -a
  undone to *, before undo -a -n 25 -f (glob)

hg undo --keep tests
  $ touch kfl1 && hg add kfl1
  $ hg st
  A kfl1
  $ hg commit --amend
  $ hg st
  $ hg undo --keep
  undone to *, before commit --amend (glob)
  hint[undo-uncommit-unamend]: undoing amends discards their changes.
  to restore the changes to the working copy, run 'hg revert -r c9476255bc2a --all'
  in the future, you can use 'hg unamend' instead of 'hg undo' to keep changes
  hint[hint-ack]: use 'hg hint --ack undo-uncommit-unamend' to silence these hints
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
  > amend=
  > smartlog=
  > tweakdefaults=
  > EOF
  $ hg undo
  undone to *, before commit --amend (glob)
  hint[undo-uncommit-unamend]: undoing amends discards their changes.
  to restore the changes to the working copy, run 'hg revert -r c9476255bc2a --all'
  in the future, you can use 'hg unamend' instead of 'hg undo' to keep changes
  hint[hint-ack]: use 'hg hint --ack undo-uncommit-unamend' to silence these hints

# Make '.' public
  $ hg debugremotebookmark foo .

(With visible head-based undo, 'undone to' template is not shown)
(UX: Consider using 'x' for secret phase commits, instead of using 'obsolete()')

  $ hg log -Gr 'predecessors(all()+f007a7cf4c3d+aa430c8afedf+c9476255bc2a+2dca609174c2+db92053d5c83)' -T "{node|short} {phase}"
  o  c9476255bc2a secret
  |
  | o  f007a7cf4c3d secret
  | |
  +---o  2dca609174c2 secret
  | |
  | | o  aa430c8afedf secret
  | |/
  | @  1dafc0b43612 public
  |/
  o  0a3dd3e15e65 public
  |
  | o  296fda51a303 draft
  |/
  | o  db92053d5c83 secret
  |/
  o  38d85b506754 public
  |
  o  ec7553f7b382 public
  |
  | o  49cdb4091aca draft
  |/
  o  b68836a6e2ca public
  |
  o  df4fd610a3d6 public
  
# Make everything draft
  $ hg debugremotebookmark foo null

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
  $ hg undo
  undone to *, before split --quiet (glob)
  $ hg undo -n -1
  undone to *, before undo (glob)
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend=!
  > smartlog=!
  > tweakdefaults=!
  > EOF

File corruption handling
  $ echo 111corruptedrevlog > .hg/undolog/index.i
  $ hg up . --debug
  caught revlog error. undolog/index.i was probably corrupted
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugundohistory -l
  0:  -- gap in log -- 

hg undo --preview test
  $ touch prev1 && hg add prev1 && hg ci -m prev1
  $ cat >> $HGRCPATH <<EOF
  > [templatealias]
  > undopreview = '{if(undonecommits(UNDOINDEX), "Undone")}'
  > EOF
  $ hg undo -p
  @  Undone
  |
  o
  |
  o
  |
  o
  |
  o
  |
  o
  |
  o
  |
  o
  |
  o
  
  undo to *, before ci -m prev1 (glob)


hg redo tests

  $ newrepo
  $ setconfig ui.allowemptycommit=1 hint.ack='*'
  $ hg commit -m A
  $ hg commit -m B
  $ hg commit -m C
  $ hg log -T '{desc}'
  CBA (no-eol)

  $ hg undo -q
  $ hg log -T '{desc}'
  BA (no-eol)

  $ hg undo -q
  $ hg log -T '{desc}'
  A (no-eol)

  $ hg redo
  undone to *, before undo -q (glob)
  $ hg log -T '{desc}'
  BA (no-eol)

  $ hg undo -q
  $ hg log -T '{desc}'
  A (no-eol)

  $ hg redo -q
  $ hg log -T '{desc}'
  BA (no-eol)

  $ hg redo -q
  $ hg log -T '{desc}'
  CBA (no-eol)

  $ hg debugundohistory -l
  0: redo -q
  1: redo -q
  2: undo -q
  3: redo
  4: undo -q
