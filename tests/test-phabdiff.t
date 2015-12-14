  $ echo "[extensions]" >> $HGRCPATH
  $ echo "gitlookup = $TESTDIR/../phabdiff.py" >> $HGRCPATH

Setup repo

  $ hg init repo
  $ cd repo

Test phabdiff template mapping

  $ echo a > a
  $ hg commit -Aqm "Differential Revision: https://phabricator.fb.com/D1234
  > Tasks: 2312"
  $ hg log --template "{phabdiff}\n"
  D1234

  $ echo b > b
  $ hg commit -Aqm "Differential Revision: https://phabricator.fb.com/D5678
  > Tasks: 32, 44"
  $ hg log --template "{phabdiff}: {tasks}\n"
  D5678: T32 T44
  D1234: T2312
