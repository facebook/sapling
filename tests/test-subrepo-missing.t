  $ hg init repo
  $ cd repo
  $ hg init subrepo
  $ echo a > subrepo/a
  $ hg -R subrepo ci -Am adda
  adding a
  $ echo 'subrepo = subrepo' > .hgsub
  $ hg ci -Am addsubrepo
  adding .hgsub
  $ echo b > subrepo/b
  $ hg -R subrepo ci -Am addb
  adding b
  $ hg ci -m updatedsub

ignore blanklines in .hgsubstate

  >>> file('.hgsubstate', 'wb').write('\n\n   \t \n   \n')
  $ hg st --subrepos
  M .hgsubstate
  $ hg revert -qC .hgsubstate

abort more gracefully on .hgsubstate parsing error

  $ cp .hgsubstate .hgsubstate.old
  >>> file('.hgsubstate', 'wb').write('\ninvalid')
  $ hg st --subrepos --cwd $TESTTMP -R $TESTTMP/repo
  abort: invalid subrepository revision specifier in 'repo/.hgsubstate' line 2 (glob)
  [255]
  $ mv .hgsubstate.old .hgsubstate

delete .hgsub and revert it

  $ rm .hgsub
  $ hg revert .hgsub
  warning: subrepo spec file '.hgsub' not found
  warning: subrepo spec file '.hgsub' not found
  warning: subrepo spec file '.hgsub' not found

delete .hgsubstate and revert it

  $ rm .hgsubstate
  $ hg revert .hgsubstate

delete .hgsub and update

  $ rm .hgsub
  $ hg up 0 --cwd $TESTTMP -R $TESTTMP/repo
  warning: subrepo spec file 'repo/.hgsub' not found (glob)
  warning: subrepo spec file 'repo/.hgsub' not found (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  warning: subrepo spec file '.hgsub' not found
  ! .hgsub
  $ ls subrepo
  a

delete .hgsubstate and update

  $ hg up -C
  warning: subrepo spec file '.hgsub' not found
  warning: subrepo spec file '.hgsub' not found
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm .hgsubstate
  $ hg up 0
  remote changed .hgsubstate which local deleted
  use (c)hanged version or leave (d)eleted? c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  $ ls subrepo
  a

Enable obsolete

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate= {rev}:{node|short} {desc|firstline}
  > [phases]
  > publish=False
  > [experimental]
  > evolution=createmarkers
  > EOF

check that we can update parent repo with missing (amended) subrepo revision

  $ hg up --repository subrepo -r tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg ci -m "updated subrepo to tip"
  created new head
  $ cd subrepo
  $ hg update -r tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo foo > a
  $ hg commit --amend -m "addb (amended)"
  $ cd ..
  $ hg update --clean .
  revision 102a90ea7b4a in subrepo subrepo is hidden
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

check that --hidden is propagated to the subrepo

  $ hg -R subrepo up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg ci -m 'commit with amended subrepo'
  $ echo bar > subrepo/a
  $ hg -R subrepo ci --amend -m "amend a (again)"
  $ hg --hidden cat subrepo/a
  foo

verify will warn if locked-in subrepo revisions are hidden or missing

  $ hg ci -m "amended subrepo (again)"
  $ hg --config extensions.strip= --hidden strip -R subrepo -qr 'tip'
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 5 changesets, 5 total revisions
  checking subrepo links
  subrepo 'subrepo' is hidden in revision a66de08943b6
  subrepo 'subrepo' is hidden in revision 674d05939c1e
  subrepo 'subrepo' not found in revision a7d05d9055a4

  $ cd ..
