  $ setconfig extensions.treemanifest=!
  > mkcommit()
  > {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "add $1"
  > }

Set up extension and repos to clone over wire protocol

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > [phases]
  > publish = False
  > [extensions]
  > remotenames=
  > EOF
  $ hg init repo1
  $ hg clone  ssh://user@dummy/repo1 repo2
  no changes found
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2

Test that anonymous heads are disallowed by default

  $ echo a > a
  $ hg add a
  $ hg commit -m a
  $ hg push
  pushing to ssh://user@dummy/repo1
  searching for changes
  abort: push would create new anonymous heads (cb9a9f314b8b)
  (use --allow-anon to override this warning)
  [255]

Create a remote bookmark

  $ hg push --to @ --create
  pushing rev cb9a9f314b8b to destination ssh://user@dummy/repo1 bookmark @
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  exporting bookmark @

Test that we can still push a head that advances a remote bookmark

  $ echo b >> a
  $ hg commit -m b
  $ hg book @
  $ hg push
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  updating bookmark @

Test --delete

  $ hg push --delete @
  pushing to ssh://user@dummy/repo1
  searching for changes
  no changes found
  deleting remote bookmark @
  [1]

Test that we don't get an abort if we're doing a bare push that does nothing

  $ hg bookmark -d @
  $ hg push
  pushing to ssh://user@dummy/repo1
  searching for changes
  no changes found
  [1]

Test that we can still push a head if there are no bookmarks in either the
remote or local repo

  $ echo c >> a
  $ hg commit -m c
  $ hg push --allow-anon
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files


  $ hg log -G -T '{rev} {node|short} {bookmarks} {remotebookmarks}\n'
  @  2 2d95304fed5d
  |
  o  1 1846eede8b68
  |
  o  0 cb9a9f314b8b
  
  $ hg bookmark foo
  $ hg push -B foo
  pushing to ssh://user@dummy/repo1
  searching for changes
  no changes found
  exporting bookmark foo
  [1]
  $ hg log -G -T '{rev} {node|short} {bookmarks} {remotebookmarks}\n'
  @  2 2d95304fed5d foo default/foo
  |
  o  1 1846eede8b68
  |
  o  0 cb9a9f314b8b
  
  $ hg boo -d foo
  $ hg debugstrip . -q
  $ hg log -G -T '{rev} {node|short} {bookmarks} {remotebookmarks}\n'
  @  1 1846eede8b68
  |
  o  0 cb9a9f314b8b
  
  $ hg push
  pushing to ssh://user@dummy/repo1
  searching for changes
  no changes found
  [1]

Test pushrev configuration option

  $ echo "[remotenames]" >> $HGRCPATH
  $ echo "pushrev = ." >> $HGRCPATH
  $ echo d >> a
  $ hg commit -qm 'da'
  $ hg push
  pushing to ssh://user@dummy/repo1
  searching for changes
  abort: push would create new anonymous heads (7481df5f123a)
  (use --allow-anon to override this warning)
  [255]

Set up server repo
  $ hg init rnserver
  $ cd rnserver
  $ mkcommit a
  $ hg book -r 0 rbook
  $ cd ..

Set up client repo
  $ hg clone rnserver rnclient
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd rnclient
  $ hg book --all
  no bookmarks set
     default/rbook             0:1f0dee641bb7
  $ cd ..

Advance a server bookmark to an unknown commit and create a new server bookmark
We want to test both the advancement of locally known remote bookmark and the
creation of a new one (locally unknonw).
  $ cd rnserver
  $ mkcommit b
  $ hg book -r 1 rbook
  moving bookmark 'rbook' forward from 1f0dee641bb7
  $ hg book -r 1 rbook2
  $ hg book
     rbook                     1:7c3bad9141dc
     rbook2                    1:7c3bad9141dc
  $ cd ..

Force client to get data about new bookmarks without getting commits
  $ cd rnclient
  $ hg push
  pushing to $TESTTMP/repo2/rnserver
  searching for changes
  no changes found
  [1]
  $ hg book --all
  no bookmarks set
     default/rbook             0:1f0dee641bb7
  $ hg update rbook
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

