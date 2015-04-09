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
  (use --force to override this warning)
  [255]

Create a remote bookmark

  $ hg push --to @ -f
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
  $ hg push -f
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
