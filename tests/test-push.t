Set up extension and repos to clone over wire protocol

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > [phases]
  > publish = False
  > [extensions]
  > remotenames=`dirname $TESTDIR`/remotenames.py
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
  $ hg --config extensions.strip= strip . -q
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

Test traditional push with subrepo

  $ cd ../repo1
  $ hg init nested
  $ cd nested
  $ hg bookmark @
  $ cd ..
  $ cd ../repo2
  $ hg init nested
  $ cd nested
  $ echo a > a
  $ hg commit -qAm 'aa'
  $ hg bookmark @
  $ cd ..
  $ echo nested=nested > .hgsub
  $ hg add .hgsub
  $ hg commit -m sub
  $ hg push
  pushing to ssh://user@dummy/repo1
  pushing subrepo nested to ssh://user@dummy/repo1/nested
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  updating bookmark @
  searching for changes
  abort: push would create new anonymous heads (296c645d2a63)
  (use --allow-anon to override this warning)
  [255]
  $ hg bookmark @
  $ hg push
  pushing to ssh://user@dummy/repo1
  no changes made to subrepo nested since last push to ssh://user@dummy/repo1/nested
  searching for changes
  abort: push would create new anonymous heads (296c645d2a63)
  (use --allow-anon to override this warning)
  [255]
  $ hg push --to @
  pushing rev 296c645d2a63 to destination ssh://user@dummy/repo1 bookmark @
  searching for changes
  abort: not creating new remote bookmark
  (use --create to create a new bookmark)
  [255]
  $ hg push -B @
  pushing to ssh://user@dummy/repo1
  no changes made to subrepo nested since last push to ssh://user@dummy/repo1/nested
  searching for changes
  remote has heads on branch 'default' that are not known locally: 2d95304fed5d
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 3 changes to 3 files (+1 heads)
  exporting bookmark @

