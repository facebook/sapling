
  $ mkcommit() {
  >  echo "$1" > "$1"
  >  hg add "$1"
  >  hg ci -m "$1"
  > }

  $ extpath=`dirname $TESTDIR`
  $ . $TESTDIR/require-ext.sh evolve
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastpartialmatch=$extpath/hgext3rd/fastpartialmatch.py
  > strip=
  > histedit=
  > evolve=
  > [experimental]
  > evolution=createmarkers
  > evolutioncommands=obsolete
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF

  $ hg init repo
  $ cd repo
  $ mkcommit firstcommit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 000000000000
  1 changesets pruned
  $ hg debugrebuildpartialindex
  $ hg debugcheckpartialindex
  $ mkcommit first
  $ hg debugcheckpartialindex
