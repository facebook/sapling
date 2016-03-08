  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > histedit=
  > EOF

  $ initrepos ()
  > {
  >     hg init r
  >     cd r
  >     for x in a b c ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  >     cd ..
  >     hg clone r r2 | grep -v updating
  >     cd r2
  >     for x in d e f ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  >     cd ..
  >     hg init r3
  >     cd r3
  >     for x in g h i ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  >     cd ..
  > }

  $ initrepos
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

show the edit commands offered by outgoing
  $ cd r2
  $ HGEDITOR=cat hg histedit --outgoing ../r | grep -v comparing | grep -v searching
  pick 055a42cdd887 3 d
  pick e860deea161a 4 e
  pick 652413bf663e 5 f
  
  # Edit history between 055a42cdd887 and 652413bf663e
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
  $ cd ..

show the error from unrelated repos
  $ cd r3
  $ HGEDITOR=cat hg histedit --outgoing ../r | grep -v comparing | grep -v searching
  abort: repository is unrelated
  [1]
  $ cd ..

show the error from unrelated repos
  $ cd r3
  $ HGEDITOR=cat hg histedit --force --outgoing ../r
  comparing with ../r
  searching for changes
  warning: repository is unrelated
  pick 2a4042b45417 0 g
  pick 68c46b4927ce 1 h
  pick 51281e65ba79 2 i
  
  # Edit history between 2a4042b45417 and 51281e65ba79
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
  $ cd ..

test sensitivity to branch in URL:

  $ cd r2
  $ hg -q update 2
  $ hg -q branch foo
  $ hg commit -m 'create foo branch'
  $ HGEDITOR=cat hg histedit --outgoing '../r#foo' | grep -v comparing | grep -v searching
  pick f26599ee3441 6 create foo branch
  
  # Edit history between f26599ee3441 and f26599ee3441
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

test to check number of roots in outgoing revisions

  $ hg -q outgoing -G --template '{node|short}({branch})' '../r'
  @  f26599ee3441(foo)
  
  o  652413bf663e(default)
  |
  o  e860deea161a(default)
  |
  o  055a42cdd887(default)
  
  $ HGEDITOR=cat hg -q histedit --outgoing '../r'
  abort: there are ambiguous outgoing revisions
  (see "hg help histedit" for more detail)
  [255]

  $ hg -q update -C 2
  $ echo aa >> a
  $ hg -q commit -m 'another head on default'
  $ hg -q outgoing -G --template '{node|short}({branch})' '../r#default'
  @  3879dc049647(default)
  
  o  652413bf663e(default)
  |
  o  e860deea161a(default)
  |
  o  055a42cdd887(default)
  
  $ HGEDITOR=cat hg -q histedit --outgoing '../r#default'
  abort: there are ambiguous outgoing revisions
  (see "hg help histedit" for more detail)
  [255]

  $ cd ..
