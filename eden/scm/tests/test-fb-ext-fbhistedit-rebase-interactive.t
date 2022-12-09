#chg-compatible
#debugruntest-compatible

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable fbhistedit histedit rebase

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
  >     hg goto 1
  >     addcommits g h i
  >     hg goto 1
  >     echo CONFLICT > f
  >     hg add f
  >     hg ci -m "conflict f"
  >     hg goto 9
  > }

  $ initrepo
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

log before rebase

  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  8d0611d6e5f2 conflict f
  │
  │ o  cf7e1bc6a982 i
  │ │
  │ o  7523912c6e49 h
  │ │
  │ o  0ba40a7dd69a g
  ├─╯
  │ o  652413bf663e f
  │ │
  │ o  e860deea161a e
  │ │
  │ o  055a42cdd887 d
  │ │
  │ o  177f92b77385 c
  ├─╯
  o  d2ae7f538514 b
  │
  o  cb9a9f314b8b a
  
Simple rebase with -s and -d

  $ hg goto cf7e1bc6a982390237dd47e096c15bca92fe2237
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ HGEDITOR=true hg rebase -i -s cf7e1bc6a982390237dd47e096c15bca92fe2237 -d 652413bf663ef2a641cab26574e46d5f5a64a55a

  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  bb8affa27bd8 i
  │
  │ o  8d0611d6e5f2 conflict f
  │ │
  │ │ o  7523912c6e49 h
  │ │ │
  │ │ o  0ba40a7dd69a g
  │ ├─╯
  o │  652413bf663e f
  │ │
  o │  e860deea161a e
  │ │
  o │  055a42cdd887 d
  │ │
  o │  177f92b77385 c
  ├─╯
  o  d2ae7f538514 b
  │
  o  cb9a9f314b8b a
  

Try to rebase with conflict (also check -d without -s)
  $ hg goto 'desc("conflict f")'
  1 files updated, 0 files merged, 4 files removed, 0 files unresolved

  $ HGEDITOR=true hg rebase -i -d 'desc(i)'
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

  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  b6ca70f8129d conflict f
  │
  o  bb8affa27bd8 i
  │
  │ o  7523912c6e49 h
  │ │
  │ o  0ba40a7dd69a g
  │ │
  o │  652413bf663e f
  │ │
  o │  e860deea161a e
  │ │
  o │  055a42cdd887 d
  │ │
  o │  177f92b77385 c
  ├─╯
  o  d2ae7f538514 b
  │
  o  cb9a9f314b8b a
  

Rebase with base
  $ hg goto 'desc(h)'
  2 files updated, 0 files merged, 5 files removed, 0 files unresolved
  $ HGEDITOR=true hg rebase -i -b . -d 'desc(conflict)'
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  50cf975d06ef h
  │
  o  ba6932766227 g
  │
  o  b6ca70f8129d conflict f
  │
  o  bb8affa27bd8 i
  │
  o  652413bf663e f
  │
  o  e860deea161a e
  │
  o  055a42cdd887 d
  │
  o  177f92b77385 c
  │
  o  d2ae7f538514 b
  │
  o  cb9a9f314b8b a
  
Rebase with -s and -d and checked out to something that is not a child of
either the source or destination.  This unfortunately is rejected since the
histedit code currently requires all edited commits to be ancestors of the
current working directory parent.

  $ hg goto 'desc(i) - desc(conflict)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ addcommits x y z
  $ hg goto 'desc(f) - desc(conflict)'
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  o  70ff95fe5c79 z
  │
  o  9843e524084d y
  │
  o  a5ae87083656 x
  │
  │ o  50cf975d06ef h
  │ │
  │ o  ba6932766227 g
  │ │
  │ o  b6ca70f8129d conflict f
  ├─╯
  o  bb8affa27bd8 i
  │
  @  652413bf663e f
  │
  o  e860deea161a e
  │
  o  055a42cdd887 d
  │
  o  177f92b77385 c
  │
  o  d2ae7f538514 b
  │
  o  cb9a9f314b8b a
  
  $ HGEDITOR=true hg rebase -i -s 'desc(y)' -d 'desc(g)'
  abort: source revision (-s) must be an ancestor of the working directory for interactive rebase
  [255]
