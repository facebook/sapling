#chg-compatible
#debugruntest-compatible

  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable histedit

  $ hg init r
  $ cd r

  $ for x in a b c d e f ; do
  >     echo $x > $x
  >     hg add $x
  >     hg ci -m $x
  > done

  $ hg book -r 'desc(b)' will-move-backwards
  $ hg book -r 'desc(c)' two
  $ hg book -r 'desc(c)' also-two
  $ hg book -r 'desc(d)' three
  $ hg book -r 'desc(e)' four
  $ hg book -r tip five
  $ hg log --graph
  @  commit:      652413bf663e
  │  bookmark:    five
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     f
  │
  o  commit:      e860deea161a
  │  bookmark:    four
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     e
  │
  o  commit:      055a42cdd887
  │  bookmark:    three
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     d
  │
  o  commit:      177f92b77385
  │  bookmark:    also-two
  │  bookmark:    two
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     c
  │
  o  commit:      d2ae7f538514
  │  bookmark:    will-move-backwards
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     b
  │
  o  commit:      cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ HGEDITOR=cat hg histedit 'max(desc(b))'
  pick d2ae7f538514 b
  pick 177f92b77385 c
  pick 055a42cdd887 d
  pick e860deea161a e
  pick 652413bf663e f
  
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
  #  b, base = checkout changeset and apply further changesets from there
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description and date
  #
  $ hg histedit 'max(desc(b))' --commands - --verbose << EOF | grep histedit
  > pick 177f92b77385 2 c
  > drop d2ae7f538514 1 b
  > pick 055a42cdd887 3 d
  > fold e860deea161a 4 e
  > pick 652413bf663e 5 f
  > EOF
  [1]
  $ hg log --graph
  @  commit:      cacdfd884a93
  │  bookmark:    five
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     f
  │
  o  commit:      59d9f330561f
  │  bookmark:    four
  │  bookmark:    three
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     d
  │
  o  commit:      b346ab9a313d
  │  bookmark:    also-two
  │  bookmark:    two
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     c
  │
  o  commit:      cb9a9f314b8b
     bookmark:    will-move-backwards
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ HGEDITOR=cat hg histedit 'max(desc(c))'
  pick b346ab9a313d c
  pick 59d9f330561f d
  pick cacdfd884a93 f
  
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
  #  b, base = checkout changeset and apply further changesets from there
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description and date
  #
  $ hg histedit 'max(desc(c))' --commands - --verbose << EOF | grep histedit
  > pick b346ab9a313d 1 c
  > pick cacdfd884a93 3 f
  > pick 59d9f330561f 2 d
  > EOF
  [1]

We expect 'five' to stay at tip, since the tipmost bookmark is most
likely the useful signal.

  $ hg log --graph
  @  commit:      c04e50810e4b
  │  bookmark:    five
  │  bookmark:    four
  │  bookmark:    three
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     d
  │
  o  commit:      c13eb81022ca
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     f
  │
  o  commit:      b346ab9a313d
  │  bookmark:    also-two
  │  bookmark:    two
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     c
  │
  o  commit:      cb9a9f314b8b
     bookmark:    will-move-backwards
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
