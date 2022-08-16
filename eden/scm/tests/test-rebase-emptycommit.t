#chg-compatible
#debugruntest-compatible

  $ configure mutation-norecord
  $ enable rebase

  $ hg init non-merge
  $ cd non-merge
  $ hg debugdrawdag<<'EOS'
  >   F
  >   |
  >   E
  >   |  # bookmark BOOK-F=F
  >   D  # bookmark BOOK-E=E
  >   |  # bookmark BOOK-D=D
  > B C  # bookmark BOOK-C=C
  > |/
  > A
  > EOS

  $ hg debugdrawdag<<'EOS'
  > E
  > |
  > D
  > |
  > B
  > EOS
  $ hg book -d A B C D E F

  $ hg log -G -T '{desc} {bookmarks}'
  o  E
  │
  o  D
  │
  │ o  F BOOK-F
  │ │
  │ o  E BOOK-E
  │ │
  │ o  D BOOK-D
  │ │
  │ o  C BOOK-C
  │ │
  o │  B
  ├─╯
  o  A
  
With --keep, bookmark should move

  $ hg rebase -r 'e7b3f00ed42ef8977173765eccff8a861809549b+"BOOK-E"' -d 'max(desc(E))' --keep
  rebasing e7b3f00ed42e "D" (BOOK-D)
  note: rebase of e7b3f00ed42e created no changes to commit
  rebasing 69a34c08022a "E" (BOOK-E)
  note: rebase of 69a34c08022a created no changes to commit
  $ hg log -G -T '{desc} {bookmarks}'
  o  E BOOK-D BOOK-E
  │
  o  D
  │
  │ o  F BOOK-F
  │ │
  │ o  E
  │ │
  │ o  D
  │ │
  │ o  C BOOK-C
  │ │
  o │  B
  ├─╯
  o  A
  
Move D and E back for the next test

  $ hg bookmark BOOK-D -fqir e7b3f00ed42ef8977173765eccff8a861809549b
  $ hg bookmark BOOK-E -fqir 69a34c08022af689d8a6e9be8d266f91f0cc79ec

Bookmark is usually an indication of a head. For changes that are introduced by
an ancestor of bookmark B, after moving B to B-NEW, the changes are ideally
still introduced by an ancestor of changeset on B-NEW. In the below case,
"BOOK-D", and "BOOK-E" include changes introduced by "C".

  $ hg rebase -s 'desc(C)' -d 'max(desc(E))'
  rebasing dc0947a82db8 "C" (BOOK-C)
  rebasing e7b3f00ed42e "D" (BOOK-D)
  note: rebase of e7b3f00ed42e created no changes to commit
  rebasing 69a34c08022a "E" (BOOK-E)
  note: rebase of 69a34c08022a created no changes to commit
  rebasing 6b2aeab91270 "F" (BOOK-F)
  $ hg log -G -T '{desc} {bookmarks}'
  o  F BOOK-F
  │
  o  C BOOK-C BOOK-D BOOK-E
  │
  o  E
  │
  o  D
  │
  o  B
  │
  o  A
  
Merge and its ancestors all become empty

  $ hg init $TESTTMP/merge1
  $ cd $TESTTMP/merge1

  $ hg debugdrawdag<<'EOS'
  >     E
  >    /|
  > B C D   # bookmark BOOK-E=E
  >  \|/    # bookmark BOOK-D=D
  >   A     # bookmark BOOK-C=C
  > EOS

  $ hg debugdrawdag<<'EOS'
  > H
  > |
  > D
  > |
  > C
  > |
  > B
  > EOS
  $ hg book -d A B C D E

  $ hg rebase -r '(desc(A)::)-(desc(B)::)-desc(A)' -d 'desc(H)'
  rebasing b18e25de2cf5 "D" (BOOK-D)
  note: rebase of b18e25de2cf5 created no changes to commit
  rebasing dc0947a82db8 "C" (BOOK-C)
  note: rebase of dc0947a82db8 created no changes to commit
  rebasing 86a1f6686812 "E" (BOOK-E)
  note: rebase of 86a1f6686812 created no changes to commit

  $ hg log -G -T '{desc} {bookmarks}'
  o  H BOOK-C BOOK-D BOOK-E H
  │
  o  D
  │
  o  C
  │
  o  B
  │
  o  A
  
Part of ancestors of a merge become empty

  $ hg init $TESTTMP/merge2
  $ cd $TESTTMP/merge2

  $ hg debugdrawdag<<'EOS'
  >     G
  >    /|
  >   E F  # bookmark BOOK-G=G
  >   | |  # bookmark BOOK-F=F
  > B C D  # bookmark BOOK-E=E
  >  \|/   # bookmark BOOK-D=D
  >   A    # bookmark BOOK-C=C
  > EOS

  $ hg debugdrawdag<<'EOS'
  > H
  > |
  > F
  > |
  > C
  > |
  > B
  > EOS
  $ hg book -d A B C D E F G H

  $ hg rebase -r '(desc(A)::)-(desc(B)::)-desc(A)' -d 'desc(H)'
  rebasing dc0947a82db8 "C" (BOOK-C)
  note: rebase of dc0947a82db8 created no changes to commit
  rebasing 03ca77807e91 "E" (BOOK-E)
  rebasing b18e25de2cf5 "D" (BOOK-D)
  rebasing ad6717a6a58e "F" (BOOK-F)
  note: rebase of ad6717a6a58e created no changes to commit
  rebasing c58e8bdac1f4 "G" (BOOK-G)

  $ hg log -G -T '{desc} {bookmarks}'
  o    G BOOK-G
  ├─╮
  │ o  D BOOK-D BOOK-F
  │ │
  o │  E BOOK-E
  ├─╯
  o  H BOOK-C
  │
  o  F
  │
  o  C
  │
  o  B
  │
  o  A
  
