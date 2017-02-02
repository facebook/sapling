
  $ mkcommit() {
  >  echo "$1" > "$1"
  >  hg add "$1"
  >  hg ci -m "$1"
  > }

  $ extpath=`dirname $TESTDIR`
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastpartialmatch=$extpath/hgext3rd/fastpartialmatch.py
  > strip=
  > histedit=
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF

  $ hg init repo
  $ cd repo
  $ hg debugrebuildpartialindex
  $ mkcommit "first"
  $ hg debugcheckpartialindex
  $ hg log -r . -T '{node}\n'
  b75a450e74d5a7708da8c3144fbeb4ac88694044

Check debug commands
  $ hg debugrebuildpartialindex
  $ hg debugcheckpartialindex
  $ hg debugprintpartialindexfile
  abort: please specify a filename
  [255]
  $ hg debugprintpartialindexfile unknownfile
  file unknownfile does not exist
  $ hg debugprintpartialindexfile b7
  b75a450e74d5a7708da8c3144fbeb4ac88694044 0

Check that debugcheckpartialindex fails on corrupted indexes
  $ hg debugcheckpartialindex
  $ rm .hg/store/partialindex/b7
  $ hg debugcheckpartialindex
  b75a450e74d5a7708da8c3144fbeb4ac88694044 node not found in partialindex
  [1]
  $ printf 'garbage' > .hg/store/partialindex/b7
  $ hg debugcheckpartialindex
  b7 file is corrupted: corrupted index
  b75a450e74d5a7708da8c3144fbeb4ac88694044 node not found in partialindex
  [1]
  $ mkcommit committostrip
  $ hg log -r . -T '{node}'
  1138fa1e0b22411fc96c825c2603c5c3d056a206 (no-eol)
  $ hg debugrebuildpartialindex
  $ mv .hg/store/partialindex .hg/store/tmppartialindex
  $ hg strip .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/1138fa1e0b22-27b827b8-backup.hg (glob)
  $ mv .hg/store/tmppartialindex .hg/store/partialindex
  $ hg debugcheckpartialindex
  abort: 00changelog.i@1138fa1e0b22: no node!
  [255]

  $ hg debugrebuildpartialindex
  $ hg debugcheckpartialindex
