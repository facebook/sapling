
  $ enable obsstore
  $ mkcommit() {
  >  echo "$1" > "$1"
  >  hg add "$1"
  >  hg ci -m "$1"
  > }

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastpartialmatch=
  > obsshelve=
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF

  $ hg init repo
  $ cd repo
  $ echo 1 > 1
  $ echo 2 > 2
  $ hg add 1 2
  $ hg ci -m 'first'
  $ echo 3 > 2
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit second
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing * "shelve changes to: first" (glob)
