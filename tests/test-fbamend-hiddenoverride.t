  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > drawdag=$RUNTESTDIR/drawdag.py
  > [experimental]
  > evolution = all
  > EOF

  $ hg init
  $ hg debugdrawdag <<'EOS'
  > B C   # amend: B -> C
  > |/
  > A
  > EOS

  $ rm .hg/localtags
  $ hg log -G -T '{rev} {desc}\n'
  o  2 C
  |
  o  0 A
  
  $ hg log -G -T '{rev} {desc}\n' --hidden
  o  2 C
  |
  | x  1 B
  |/
  o  0 A
  
Changing working copy parent pins a node

  $ hg update 1 --hidden -q
  $ hg update 0 -q
  $ hg log -G -T '{rev} {desc}\n'
  o  2 C
  |
  | x  1 B
  |/
  @  0 A
  
Strip/prune unpins a node

  $ hg prune 1 -q
  $ hg log -G -T '{rev} {desc}\n'
  o  2 C
  |
  @  0 A
  
Bookmark pins nodes even after removed

  $ hg bookmark -ir 1 BOOK --hidden -q
  $ hg bookmark -d BOOK -q
  $ hg log -G -T '{rev} {desc}\n'
  o  2 C
  |
  | x  1 B
  |/
  @  0 A
  
