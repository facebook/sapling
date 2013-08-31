  $ "$TESTDIR/hghave" symlink || exit 80

Create extension that can disable symlink support:

  $ cat > nolink.py <<EOF
  > from mercurial import extensions, util
  > def setflags(orig, f, l, x):
  >     pass
  > def checklink(orig, path):
  >     return False
  > def extsetup(ui):
  >     extensions.wrapfunction(util, 'setflags', setflags)
  >     extensions.wrapfunction(util, 'checklink', checklink)
  > EOF

  $ hg init unix-repo
  $ cd unix-repo
  $ echo foo > a
  $ ln -s a b
  $ hg ci -Am0
  adding a
  adding b
  $ cd ..

Simulate a checkout shared on NFS/Samba:

  $ hg clone -q unix-repo shared
  $ cd shared
  $ rm b
  $ echo foo > b
  $ hg --config extensions.n=$TESTTMP/nolink.py status --debug
  ignoring suspect symlink placeholder "b"

Make a clone using placeholders:

  $ hg --config extensions.n=$TESTTMP/nolink.py clone . ../win-repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../win-repo
  $ cat b
  a (no-eol)
  $ hg --config extensions.n=$TESTTMP/nolink.py st --debug

Empty placeholder:

  $ rm b
  $ touch b
  $ hg --config extensions.n=$TESTTMP/nolink.py st --debug
  ignoring suspect symlink placeholder "b"

Write binary data to the placeholder:

  >>> open('b', 'w').write('this is a binary\0')
  $ hg --config extensions.n=$TESTTMP/nolink.py st --debug
  ignoring suspect symlink placeholder "b"

Write a long string to the placeholder:

  >>> open('b', 'w').write('this' * 1000)
  $ hg --config extensions.n=$TESTTMP/nolink.py st --debug
  ignoring suspect symlink placeholder "b"

Commit shouldn't succeed:

  $ hg --config extensions.n=$TESTTMP/nolink.py ci -m1
  nothing changed
  [1]

Write a valid string to the placeholder:

  >>> open('b', 'w').write('this')
  $ hg --config extensions.n=$TESTTMP/nolink.py st --debug
  M b
  $ hg --config extensions.n=$TESTTMP/nolink.py ci -m1
  $ hg manifest tip --verbose
  644   a
  644 @ b

  $ cd ..
