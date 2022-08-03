  $ setconfig workingcopy.ruststatus=False
set up test repo

  $ hg init uncopytest
  $ cd uncopytest
  $ echo testcontents > testfile.txt
  $ mkdir testdir
  $ echo dircontents1 > testdir/dirfile1.txt
  $ echo dircontents2 > testdir/dirfile2.txt
  $ hg add -q
  $ hg commit -m "initial commit"

uncopy a single file

  $ hg copy testfile.txt copyfile.txt
  $ hg status -C copyfile.txt
  A copyfile.txt
    testfile.txt
  $ hg uncopy copyfile.txt
  $ hg status -C copyfile.txt
  A copyfile.txt
  $ rm copyfile.txt

uncopy a directory

  $ hg copy -q testdir copydir
  $ hg status -C copydir
  A copydir/dirfile1.txt
    testdir/dirfile1.txt
  A copydir/dirfile2.txt
    testdir/dirfile2.txt
  $ hg uncopy copydir
  $ hg status -C copydir
  A copydir/dirfile1.txt
  A copydir/dirfile2.txt
  $ rm -r copydir

uncopy by pattern

  $ hg copy -q testfile.txt copyfile1.txt
  $ hg copy -q testfile.txt copyfile2.txt
  $ hg status -C copyfile1.txt copyfile2.txt
  A copyfile1.txt
    testfile.txt
  A copyfile2.txt
    testfile.txt
  $ hg uncopy copyfile*
  $ hg status -C copyfile1.txt copyfile2.txt
  A copyfile1.txt
  A copyfile2.txt
  $ rm copyfile*

uncopy nonexistent file

  $ hg uncopy notfound.txt
  notfound.txt: $ENOENT$
  [1]

uncopy a file that was not copied

  $ echo othercontents > otherfile.txt
  $ hg uncopy otherfile.txt
  [1]
  $ hg status -C otherfile.txt
  ? otherfile.txt
