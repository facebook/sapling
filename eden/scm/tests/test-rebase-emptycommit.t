  $ cat >> $HGRCPATH<<EOF
  > [extensions]
  > rebase=
  > EOF

  $ hg init non-merge
  $ cd non-merge
  $ hg debugdrawdag<<'EOS'
  >   F
  >   |
  >   E
  >   |
  >   D
  >   |
  > B C
  > |/
  > A
  > EOS

  $ for i in C D E F; do
  >   hg bookmark -r $i -i BOOK-$i
  > done

  $ hg debugdrawdag<<'EOS'
  > E
  > |
  > D
  > |
  > B
  > EOS

  $ hg log -G -T '{rev} {desc} {bookmarks}'
  o  7 E
  |
  o  6 D
  |
  | o  5 F BOOK-F
  | |
  | o  4 E BOOK-E
  | |
  | o  3 D BOOK-D
  | |
  | o  2 C BOOK-C
  | |
  o |  1 B
  |/
  o  0 A
  
With --keep, bookmark should move

  $ hg rebase -r 3+4 -d E --keep
  rebasing e7b3f00ed42e "D" (BOOK-D)
  note: rebase of 3:e7b3f00ed42e created no changes to commit
  rebasing 69a34c08022a "E" (BOOK-E)
  note: rebase of 4:69a34c08022a created no changes to commit
  $ hg log -G -T '{rev} {desc} {bookmarks}'
  o  7 E BOOK-D BOOK-E
  |
  o  6 D
  |
  | o  5 F BOOK-F
  | |
  | o  4 E
  | |
  | o  3 D
  | |
  | o  2 C BOOK-C
  | |
  o |  1 B
  |/
  o  0 A
  
Move D and E back for the next test

  $ hg bookmark BOOK-D -fqir 3
  $ hg bookmark BOOK-E -fqir 4

Bookmark is usually an indication of a head. For changes that are introduced by
an ancestor of bookmark B, after moving B to B-NEW, the changes are ideally
still introduced by an ancestor of changeset on B-NEW. In the below case,
"BOOK-D", and "BOOK-E" include changes introduced by "C".

  $ hg rebase -s 2 -d E
  rebasing dc0947a82db8 "C" (BOOK-C C)
  rebasing e7b3f00ed42e "D" (BOOK-D)
  note: rebase of 3:e7b3f00ed42e created no changes to commit
  rebasing 69a34c08022a "E" (BOOK-E)
  note: rebase of 4:69a34c08022a created no changes to commit
  rebasing 6b2aeab91270 "F" (BOOK-F F)
  $ hg log -G -T '{rev} {desc} {bookmarks}'
  o  9 F BOOK-F
  |
  o  8 C BOOK-C BOOK-D BOOK-E
  |
  o  7 E
  |
  o  6 D
  |
  | x  5 F
  | |
  | x  4 E
  | |
  | x  3 D
  | |
  | x  2 C
  | |
  o |  1 B
  |/
  o  0 A
  
Merge and its ancestors all become empty

  $ hg init $TESTTMP/merge1
  $ cd $TESTTMP/merge1

  $ hg debugdrawdag<<'EOS'
  >     E
  >    /|
  > B C D
  >  \|/
  >   A
  > EOS

  $ for i in C D E; do
  >   hg bookmark -r $i -i BOOK-$i
  > done

  $ hg debugdrawdag<<'EOS'
  > H
  > |
  > D
  > |
  > C
  > |
  > B
  > EOS

  $ hg rebase -r '(A::)-(B::)-A' -d H
  rebasing dc0947a82db8 "C" (BOOK-C)
  note: rebase of 2:dc0947a82db8 created no changes to commit
  rebasing b18e25de2cf5 "D" (BOOK-D)
  note: rebase of 3:b18e25de2cf5 created no changes to commit
  rebasing 86a1f6686812 "E" (BOOK-E E)
  note: rebase of 4:86a1f6686812 created no changes to commit

  $ hg log -G -T '{rev} {desc} {bookmarks}'
  o  7 H BOOK-C BOOK-D BOOK-E
  |
  o  6 D
  |
  o  5 C
  |
  | x    4 E
  | |\
  | | x  3 D
  | | |
  | x |  2 C
  | |/
  o /  1 B
  |/
  o  0 A
  
Part of ancestors of a merge become empty

  $ hg init $TESTTMP/merge2
  $ cd $TESTTMP/merge2

  $ hg debugdrawdag<<'EOS'
  >     G
  >    /|
  >   E F
  >   | |
  > B C D
  >  \|/
  >   A
  > EOS

  $ for i in C D E F G; do
  >   hg bookmark -r $i -i BOOK-$i
  > done

  $ hg debugdrawdag<<'EOS'
  > H
  > |
  > F
  > |
  > C
  > |
  > B
  > EOS

  $ hg rebase -r '(A::)-(B::)-A' -d H
  rebasing dc0947a82db8 "C" (BOOK-C)
  note: rebase of 2:dc0947a82db8 created no changes to commit
  rebasing b18e25de2cf5 "D" (BOOK-D D)
  rebasing 03ca77807e91 "E" (BOOK-E E)
  rebasing ad6717a6a58e "F" (BOOK-F)
  note: rebase of 5:ad6717a6a58e created no changes to commit
  rebasing c58e8bdac1f4 "G" (BOOK-G G)

  $ hg log -G -T '{rev} {desc} {bookmarks}'
  o    12 G BOOK-G
  |\
  | o  11 E BOOK-E
  | |
  o |  10 D BOOK-D BOOK-F
  |/
  o  9 H BOOK-C
  |
  o  8 F
  |
  o  7 C
  |
  | x    6 G
  | |\
  | | x  5 F
  | | |
  | x |  4 E
  | | |
  | | x  3 D
  | | |
  | x |  2 C
  | |/
  o /  1 B
  |/
  o  0 A
  
