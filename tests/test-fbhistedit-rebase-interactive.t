  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [experimental]
  > histeditng=True
  > [extensions]
  > rebase=
  > histedit=
  > EOF

  $ echo "fbhistedit=$(echo $(dirname $TESTDIR))/fbhistedit.py" >> $HGRCPATH

  $ initrepo ()
  > {
  >     hg init r
  >     cd r
  >     for x in a b c d e f ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  >     hg update 1
  >     for x in g h i ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  >     hg update 1
  >     echo CONFLICT > f
  >     hg add f
  >     hg ci -m "conflict f"
  >     hg update 9
  > }

  $ initrepo
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  created new head
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  created new head
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

log before rebase

  $ hg log -G -T '{rev}:{node|short} {desc|firstline}\n'
  @  9:8d0611d6e5f2 conflict f
  |
  | o  8:cf7e1bc6a982 i
  | |
  | o  7:7523912c6e49 h
  | |
  | o  6:0ba40a7dd69a g
  |/
  | o  5:652413bf663e f
  | |
  | o  4:e860deea161a e
  | |
  | o  3:055a42cdd887 d
  | |
  | o  2:177f92b77385 c
  |/
  o  1:d2ae7f538514 b
  |
  o  0:cb9a9f314b8b a
  
Simple rebase with -s and -d

  $ hg update 8
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ HGEDITOR=true hg rebase -i -s 8 -d 5
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/cf7e1bc6a982-9ce57ee5-backup.hg (glob)

  $ hg log -G -T '{rev}:{node|short} {desc|firstline}\n'
  @  9:bb8affa27bd8 i
  |
  | o  8:8d0611d6e5f2 conflict f
  | |
  | | o  7:7523912c6e49 h
  | | |
  | | o  6:0ba40a7dd69a g
  | |/
  o |  5:652413bf663e f
  | |
  o |  4:e860deea161a e
  | |
  o |  3:055a42cdd887 d
  | |
  o |  2:177f92b77385 c
  |/
  o  1:d2ae7f538514 b
  |
  o  0:cb9a9f314b8b a
  

Try to rebase with conflict (also check -d without -s)
  $ hg update 8
  1 files updated, 0 files merged, 4 files removed, 0 files unresolved

  $ HGEDITOR=true hg rebase -i -d 9
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging f
  warning: conflicts while merging f! (edit, then use 'hg resolve --mark')
  Fix up the change and run hg histedit --continue
  [1]

  $ echo resolved > f
  $ hg resolve --mark f
  (no more unresolved files)
  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/8d0611d6e5f2-0e0da94b-backup.hg (glob)

  $ hg log -G -T '{rev}:{node|short} {desc|firstline}\n'
  @  9:b6ca70f8129d conflict f
  |
  o  8:bb8affa27bd8 i
  |
  | o  7:7523912c6e49 h
  | |
  | o  6:0ba40a7dd69a g
  | |
  o |  5:652413bf663e f
  | |
  o |  4:e860deea161a e
  | |
  o |  3:055a42cdd887 d
  | |
  o |  2:177f92b77385 c
  |/
  o  1:d2ae7f538514 b
  |
  o  0:cb9a9f314b8b a
  

Rebase with base
  $ hg update 7
  2 files updated, 0 files merged, 5 files removed, 0 files unresolved
  $ HGEDITOR=true hg rebase -i -b . -d 9
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/0ba40a7dd69a-033489e1-backup.hg (glob)
  $ hg log -G -T '{rev}:{node|short} {desc|firstline}\n'
  @  9:50cf975d06ef h
  |
  o  8:ba6932766227 g
  |
  o  7:b6ca70f8129d conflict f
  |
  o  6:bb8affa27bd8 i
  |
  o  5:652413bf663e f
  |
  o  4:e860deea161a e
  |
  o  3:055a42cdd887 d
  |
  o  2:177f92b77385 c
  |
  o  1:d2ae7f538514 b
  |
  o  0:cb9a9f314b8b a
  
