
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > phrevset=$TESTDIR/../hgext3rd/phrevset.py
  > EOF
  $ hg init repo
  $ cd repo
  $ echo 1 > 1
  $ hg add 1
  $ hg commit -m 'Differential Revision: http.ololo.com/D1234'
  $ hg up -q 0
  $ hg up D1234
  phrevset.callsign is not set - doing a linear search
  This will be slow if the diff was not committed recently
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
