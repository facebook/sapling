#chg-compatible

TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fbhistedit=
  > histedit=
  > rebase=
  > EOF

  $ addcommits ()
  > {
  >     for x in "$@" ; do
  >         echo "$x" > "$x"
  >         hg add "$x"
  >         hg ci -m "$x"
  >     done
  > }
  $ initrepo ()
  > {
  >     hg init r
  >     cd r
  >     addcommits a b c d e f
  >     hg update 1
  >     addcommits g h i
  >     hg update 1
  >     echo CONFLICT > f
  >     hg add f
  >     hg ci -m "conflict f"
  >     hg update 9
  > }

  $ initrepo
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
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
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/cf7e1bc6a982-9ce57ee5-histedit.hg (glob)

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
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 8d0611d6e5f2)
  (hg histedit --continue to resume)
  [1]

  $ echo resolved > f
  $ hg resolve --mark f
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/8d0611d6e5f2-0e0da94b-histedit.hg (glob)

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
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/0ba40a7dd69a-033489e1-histedit.hg (glob)
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
  
Rebase with -s and -d and checked out to something that is not a child of
either the source or destination.  This unfortunately is rejected since the
histedit code currently requires all edited commits to be ancestors of the
current working directory parent.

  $ hg update 6
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ addcommits x y z
  $ hg update 5
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ hg log -G -T '{rev}:{node|short} {desc|firstline}\n'
  o  12:70ff95fe5c79 z
  |
  o  11:9843e524084d y
  |
  o  10:a5ae87083656 x
  |
  | o  9:50cf975d06ef h
  | |
  | o  8:ba6932766227 g
  | |
  | o  7:b6ca70f8129d conflict f
  |/
  o  6:bb8affa27bd8 i
  |
  @  5:652413bf663e f
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
  
  $ HGEDITOR=true hg rebase -i -s 11 -d 8
  abort: source revision (-s) must be an ancestor of the working directory for interactive rebase
  [255]
