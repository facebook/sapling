Tests for the journal extension integration with remotenames.

Skip if we can't import remotenames

  $ if [ -n $PYTHON -c 'import remotenames' 2> /dev/null ]; then
  >     echo 'skipped: missing feature: remotenames'
  >     exit 80
  > fi

  $ cat >> testmocks.py << EOF
  > # mock out util.getuser() and util.makedate() to supply testable values
  > import os
  > from mercurial import util
  > def mockgetuser():
  >     return 'foobar'
  > 
  > def mockmakedate():
  >     filename = os.path.join(os.environ['TESTTMP'], 'testtime')
  >     try:
  >         with open(filename, 'rb') as timef:
  >             time = float(timef.read()) + 1
  >     except IOError:
  >         time = 0.0
  >     with open(filename, 'wb') as timef:
  >         timef.write(str(time))
  >     return (time, 0)
  > 
  > util.getuser = mockgetuser
  > util.makedate = mockmakedate
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > journal=`dirname $TESTDIR`/journal.py
  > testmocks=`pwd`/testmocks.py
  > remotenames=
  > [remotenames]
  > rename.default=remote
  > EOF

  $ hg init remote
  $ cd remote
  $ touch a
  $ hg commit -A -m 'a commit' -q
  $ hg book bmwillnotmove
  $ hg book bm

Test journal with remote bookmarks works on clone

  $ cd ..
  $ hg clone remote local -q
  $ cd local
  $ hg journal remote/bm
  Previous locations of 'remote/bm':
  94cf1ae9e2c8  clone remote local -q

Test journal with remote bookmarks works on pull

  $ cd ../remote
  $ hg up bm -q
  $ echo 'modified' > a
  $ hg commit -m 'a second commit' -q
  $ cd ../local
  $ hg pull -q
  $ hg journal remote/bm
  Previous locations of 'remote/bm':
  b720e98e7160  pull -q
  94cf1ae9e2c8  clone remote local -q

Test journal with remote bookmarks works after push

  $ hg up remote/bm -q
  $ echo 'modified locally' > a
  $ hg commit -m 'local commit' -q
  $ hg push --to bm -q
  $ hg journal remote/bm
  Previous locations of 'remote/bm':
  869ef7e9b417  push --to bm -q
  b720e98e7160  pull -q
  94cf1ae9e2c8  clone remote local -q

Test second remotebookmark has not been clobbered or has moved since clone

  $ hg journal remote/bmwillnotmove
  Previous locations of 'remote/bmwillnotmove':
  94cf1ae9e2c8  clone remote local -q

