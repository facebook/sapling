Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > rebase=
  > [experimental]
  > evolution = createmarkers
  > EOF
  $ hg init repo && cd repo

Perform restack without inhibit extension.
  $ hg debugbuilddag -m +3
  $ hg update 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "Amended"
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
  $ hg rebase --restack
  rebasing 2:* "r2" (glob)
