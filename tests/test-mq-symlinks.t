  $ "$TESTDIR/hghave" symlink || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init
  $ hg qinit
  $ hg qnew base.patch
  $ echo aaa > a
  $ echo bbb > b
  $ echo ccc > c
  $ hg add a b c
  $ hg qrefresh
  $ $TESTDIR/readlink.py a
  a -> a not a symlink


test replacing a file with a symlink

  $ hg qnew symlink.patch
  $ rm a
  $ ln -s b a
  $ hg qrefresh --git
  $ $TESTDIR/readlink.py a
  a -> b

  $ hg qpop
  popping symlink.patch
  now at: base.patch
  $ hg qpush
  applying symlink.patch
  now at: symlink.patch
  $ $TESTDIR/readlink.py a
  a -> b


test updating a symlink

  $ rm a
  $ ln -s c a
  $ hg qnew --git -f updatelink
  $ $TESTDIR/readlink.py a
  a -> c
  $ hg qpop
  popping updatelink
  now at: symlink.patch
  $ hg qpush --debug
  applying updatelink
  patching file a
  a
  now at: updatelink
  $ $TESTDIR/readlink.py a
  a -> c
  $ hg st


test replacing a symlink with a file

  $ ln -s c s
  $ hg add s
  $ hg qnew --git -f addlink
  $ rm s
  $ echo sss > s
  $ hg qnew --git -f replacelinkwithfile
  $ hg qpop
  popping replacelinkwithfile
  now at: addlink
  $ hg qpush
  applying replacelinkwithfile
  now at: replacelinkwithfile
  $ cat s
  sss
  $ hg st


test symlink removal

  $ hg qnew removesl.patch
  $ hg rm a
  $ hg qrefresh --git
  $ hg qpop
  popping removesl.patch
  now at: replacelinkwithfile
  $ hg qpush
  applying removesl.patch
  now at: removesl.patch
  $ hg st -c
  C b
  C c
  C s

replace broken symlink with another broken symlink

  $ ln -s linka linka
  $ hg add linka
  $ hg qnew link
  $ hg mv linka linkb
  $ rm linkb
  $ ln -s linkb linkb
  $ hg qnew movelink
  $ hg qpop
  popping movelink
  now at: link
  $ hg qpush
  applying movelink
  now at: movelink
  $ $TESTDIR/readlink.py linkb
  linkb -> linkb
