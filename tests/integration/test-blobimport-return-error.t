  $ . $TESTDIR/library.sh

setup repo

  $ hg init repo-hg

Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > remotefilelog=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

  $ touch a
  $ hg add a
  $ hg ci -ma
  $ touch b
  $ hg add b
  $ hg ci -mb
  $ hg log
  changeset:   1:0e067c57feba
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ cd $TESTTMP

blobimport with missing first commit, it should fail
  $ blobimport --panic-fate=exit repo-hg/.hg repo --skip 1 > out.txt
  [101]
  $ grep PANIC < out.txt
  PANIC: cannot open revlogrepo: Os { code: 2, kind: NotFound, message: "No such file or directory" }
