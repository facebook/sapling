  $ "$TESTDIR/hghave" symlink || exit 80

  $ origdir=`pwd`

  $ hg init repo
  $ cd repo
  $ ln -s nothing dangling

avoid tar warnings about old timestamp

  $ hg ci -d '2000-01-01 00:00:00 +0000' -qAm 'add symlink'

  $ hg archive -t files ../archive
  $ hg archive -t tar -p tar ../archive.tar
  $ hg archive -t zip -p zip ../archive.zip

files

  $ cd "$origdir"
  $ cd archive
  $ "$TESTDIR/readlink.py" dangling
  dangling -> nothing

tar

  $ cd "$origdir"
  $ tar xf archive.tar
  $ cd tar
  $ "$TESTDIR/readlink.py" dangling
  dangling -> nothing

zip

  $ cd "$origdir"
  $ unzip archive.zip > /dev/null
  $ cd zip
  $ "$TESTDIR/readlink.py" dangling
  dangling -> nothing

  $ cd ..
