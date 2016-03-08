  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > histedit=
  > EOF

  $ hg init r
  $ cd r

  $ for x in a b c d e f ; do
  >     echo $x > $x
  >     hg add $x
  >     hg ci -m $x
  > done

  $ hg book -r 1 will-move-backwards
  $ hg book -r 2 two
  $ hg book -r 2 also-two
  $ hg book -r 3 three
  $ hg book -r 4 four
  $ hg book -r tip five
  $ hg log --graph
  @  changeset:   5:652413bf663e
  |  bookmark:    five
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:e860deea161a
  |  bookmark:    four
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:055a42cdd887
  |  bookmark:    three
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  bookmark:    also-two
  |  bookmark:    two
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  bookmark:    will-move-backwards
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ HGEDITOR=cat hg histedit 1
  pick d2ae7f538514 1 b
  pick 177f92b77385 2 c
  pick 055a42cdd887 3 d
  pick e860deea161a 4 e
  pick 652413bf663e 5 f
  
  # Edit history between d2ae7f538514 and 652413bf663e
  #
  # Commits are listed from least to most recent
  #
  # You can reorder changesets by reordering the lines
  #
  # Commands:
  #
  #  e, edit = use commit, but stop for amending
  #  m, mess = edit commit message without changing commit content
  #  p, pick = use commit
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #
  $ hg histedit 1 --commands - --verbose << EOF | grep histedit
  > pick 177f92b77385 2 c
  > drop d2ae7f538514 1 b
  > pick 055a42cdd887 3 d
  > fold e860deea161a 4 e
  > pick 652413bf663e 5 f
  > EOF
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/96e494a2d553-3c6c5d92-backup.hg (glob)
  histedit: moving bookmarks also-two from 177f92b77385 to b346ab9a313d
  histedit: moving bookmarks five from 652413bf663e to cacdfd884a93
  histedit: moving bookmarks four from e860deea161a to 59d9f330561f
  histedit: moving bookmarks three from 055a42cdd887 to 59d9f330561f
  histedit: moving bookmarks two from 177f92b77385 to b346ab9a313d
  histedit: moving bookmarks will-move-backwards from d2ae7f538514 to cb9a9f314b8b
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/d2ae7f538514-48787b8d-backup.hg (glob)
  $ hg log --graph
  @  changeset:   3:cacdfd884a93
  |  bookmark:    five
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   2:59d9f330561f
  |  bookmark:    four
  |  bookmark:    three
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   1:b346ab9a313d
  |  bookmark:    also-two
  |  bookmark:    two
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   0:cb9a9f314b8b
     bookmark:    will-move-backwards
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ HGEDITOR=cat hg histedit 1
  pick b346ab9a313d 1 c
  pick 59d9f330561f 2 d
  pick cacdfd884a93 3 f
  
  # Edit history between b346ab9a313d and cacdfd884a93
  #
  # Commits are listed from least to most recent
  #
  # You can reorder changesets by reordering the lines
  #
  # Commands:
  #
  #  e, edit = use commit, but stop for amending
  #  m, mess = edit commit message without changing commit content
  #  p, pick = use commit
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #
  $ hg histedit 1 --commands - --verbose << EOF | grep histedit
  > pick b346ab9a313d 1 c
  > pick cacdfd884a93 3 f
  > pick 59d9f330561f 2 d
  > EOF
  histedit: moving bookmarks five from cacdfd884a93 to c04e50810e4b
  histedit: moving bookmarks four from 59d9f330561f to c04e50810e4b
  histedit: moving bookmarks three from 59d9f330561f to c04e50810e4b
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/59d9f330561f-073008af-backup.hg (glob)

We expect 'five' to stay at tip, since the tipmost bookmark is most
likely the useful signal.

  $ hg log --graph
  @  changeset:   3:c04e50810e4b
  |  bookmark:    five
  |  bookmark:    four
  |  bookmark:    three
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:c13eb81022ca
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   1:b346ab9a313d
  |  bookmark:    also-two
  |  bookmark:    two
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   0:cb9a9f314b8b
     bookmark:    will-move-backwards
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
