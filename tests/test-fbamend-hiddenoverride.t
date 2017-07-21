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
  
The order matters - putting bookmarks or moving working copy on non-obsoleted
commits do not pin them. Test this using "debugobsolete" which will not call
"createmarkers".

Obsolete working copy, and move working copy away should make things disappear

  $ rm -rf .hg && hg init && hg debugdrawdag <<'EOS'
  > C E
  > | |
  > B D
  > |/
  > A
  > EOS

  $ hg up -q E
  $ hg debugobsolete `HGPLAIN=1 hg log -r E -T '{node}'`
  obsoleted 1 changesets
  $ hg tag --local --remove E
  $ hg log -G -T '{rev} {desc}\n'
  @  4 E
  |
  | o  3 C
  | |
  o |  2 D
  | |
  | o  1 B
  |/
  o  0 A
  
  $ hg debugobsolete `HGPLAIN=1 hg log -r D -T '{node}'`
  obsoleted 1 changesets
  $ hg tag --local --remove D
  $ hg log -G -T '{rev} {desc}\n'
  @  4 E
  |
  | o  3 C
  | |
  x |  2 D
  | |
  | o  1 B
  |/
  o  0 A
  
  $ hg update -q C
  $ hg log -G -T '{rev} {desc}\n'
  @  3 C
  |
  o  1 B
  |
  o  0 A
  
Having a bookmark on a commit, obsolete the commit, remove the bookmark

  $ rm -rf .hg && hg init && hg debugdrawdag <<'EOS'
  > C E
  > | |
  > B D
  > |/
  > A
  > EOS

  $ hg bookmark -i book-e -r E
  $ hg debugobsolete `HGPLAIN=1 hg log -r D -T '{node}'`
  obsoleted 1 changesets
  $ hg debugobsolete `HGPLAIN=1 hg log -r E -T '{node}'`
  obsoleted 1 changesets
  $ rm .hg/localtags
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  x  4 E book-e
  |
  | o  3 C
  | |
  x |  2 D
  | |
  | o  1 B
  |/
  o  0 A
  
  $ hg bookmark -d book-e
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  3 C
  |
  o  1 B
  |
  o  0 A
  
