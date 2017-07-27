  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > drawdag=$RUNTESTDIR/drawdag.py
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF

Create repo
  $ hg init
  $ hg debugdrawdag <<'EOS'
  > E
  > |
  > C D
  > |/
  > B
  > |
  > A
  > EOS
  $ rm .hg/localtags

  $ hg book -r 2 cat
  $ hg book -r 1 dog
  $ hg update 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  4 E
  |
  | o  3 D
  | |
  o |  2 C cat
  |/
  o  1 B dog
  |
  @  0 A
  

Hide a single commit
  $ hg hide 3
  1 changesets hidden
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  4 E
  |
  o  2 C cat
  |
  o  1 B dog
  |
  @  0 A
  

Hide multiple commits with bookmarks on them, hide wc parent
  $ hg update 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg hide .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 426bada5c675
  3 changesets hidden
  2 bookmarks removed
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  @  0 A
  
