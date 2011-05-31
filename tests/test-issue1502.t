http://mercurial.selenic.com/bts/issue1502

Initialize repository

  $ hg init foo
  $ touch foo/a && hg -R foo commit -A -m "added a"
  adding a

  $ hg clone foo foo1
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo "bar" > foo1/a && hg -R foo1 commit -m "edit a in foo1"
  $ echo "hi" > foo/a && hg -R foo commit -m "edited a foo"
  $ hg -R foo1 pull -u
  pulling from $TESTTMP/foo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  not updating: crosses branches (merge branches or update --check to force update)

  $ hg -R foo1 book branchy
  $ hg -R foo1 book
   * branchy                   1:e3e522925eff

Pull. Bookmark should not jump to new head.

  $ echo "there" >> foo/a && hg -R foo commit -m "edited a again"
  $ hg -R foo1 pull
  pulling from $TESTTMP/foo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

  $ hg -R foo1 book
   * branchy                   1:e3e522925eff
