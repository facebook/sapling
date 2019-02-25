Test bookmark -D
  $ cd $TESTTMP
  $ hg init book-D
  $ cd book-D
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > amend=
  > tweakdefaults=
  > [experimental]
  > evolution=all
  > EOF
  $ hg debugbuilddag '+4*2*2*2'
  $ hg bookmark -i -r 1 master
  $ hg bookmark -i -r 5 feature1
  $ hg bookmark -i -r 6 feature2
  $ hg log -G -T '{rev} {bookmarks}' -r 'all()'
  o  6 feature2
  |
  | o  5 feature1
  | |
  o |  4
  | |
  | o  3
  |/
  o  2
  |
  o  1 master
  |
  o  0
  
  $ hg bookmark -D feature1
  bookmark 'feature1' deleted
  2 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg log -G -T '{rev} {bookmarks}' -r 'all()' --hidden
  o  6 feature2
  |
  | x  5
  | |
  o |  4
  | |
  | x  3
  |/
  o  2
  |
  o  1 master
  |
  o  0
  
