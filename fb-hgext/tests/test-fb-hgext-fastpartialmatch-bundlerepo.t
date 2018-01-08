
  $ mkcommit() {
  >  echo "$1" > "$1"
  >  hg add "$1"
  >  hg ci -m "$1"
  > }

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastpartialmatch=$TESTDIR/../hgext3rd/fastpartialmatch.py
  > strip=
  > EOF

  $ hg init repo
  $ cd repo
  $ hg debugbuilddag +2
  $ hg log -r 1 -T '{node}\n'
  66f7d451a68b85ed82ff5fcc254daf50c74144bd
  $ hg strip -r 66f7d451a68b85ed8
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/66f7d451a68b-f4da9ecf-backup.hg (glob)
  $ hg debugrebuildpartialindex
  $ hg debugcheckpartialindex
  $ hg log -r 66f7d451a68b85ed8
  abort: unknown revision '66f7d451a68b85ed8'!
  [255]
  $ hg log --config fastpartialmatch.raiseifinconsistent=True -R $TESTTMP/repo/.hg/strip-backup/* -r 66f7d451a68b85ed8
  changeset:   1:66f7d451a68b
  tag:         tip
  user:        debugbuilddag
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
