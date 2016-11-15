  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "gitlookup = $TESTDIR/../hgext3rd/phabdiff.py" >> $HGRCPATH

Setup repo

  $ hg init repo
  $ cd repo

Test phabdiff template mapping

  $ echo a > a
  $ hg commit -Aqm "Differential Revision: https://phabricator.fb.com/D1234
  > Task ID: 2312"
  $ hg log --template "{phabdiff}\n"
  D1234

  $ echo c > c
  $ hg commit -Aqm "Differential Revision: http://phabricator.intern.facebook.com/D1245
  > Task ID: 2312"
  $ hg log -r . --template "{phabdiff}\n"
  D1245

  $ echo b > b
  $ hg commit -Aqm "Differential Revision: https://phabricator.fb.com/D5678
  > Tasks:32, 44"
  $ hg log -r . --template "{phabdiff}: {tasks}\n"
  D5678: 32 44
