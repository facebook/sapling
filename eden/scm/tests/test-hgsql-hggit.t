  $ . "$RUNTESTDIR/hgsql/library.sh"
  $ . "$RUNTESTDIR/hggit/testutil"
  $ shorttraceback
  $ enable lz4revlog

  $ git init a-git
  Initialized empty Git repository in $TESTTMP/a-git/.git/
  $ cd a-git

Make "a" compressable

  >>> open("a", "w").write("0\n" * 5000)
  $ git add a
  $ git commit -m a
  [master (root-commit) *] a (glob)
   1 file changed, 5000 insertions(+)
   create mode 100644 a

Setup an hgsql repo

  $ cd $TESTTMP
  $ initserver a-hg a-hg
  $ cd a-hg

Pull from git

  $ hg pull $TESTTMP/a-git
  pulling from $TESTTMP/a-git
  importing git objects into hg

