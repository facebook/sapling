#require py2
Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"
  $ setconfig hggit.mapsavefrequency=1

# Set up the git repo

  $ cd "$TESTTMP"
  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo commit1 > commit1
  $ git add .
  $ fn_git_commit -m 'commit1'

Clone the repo

  $ cd "$TESTTMP"
  $ hg clone -q -r master gitrepo hgrepo

Add more commits

  $ cd "$TESTTMP/gitrepo"
  $ echo commit2 > commit2
  $ git add .
  $ fn_git_commit -m 'commit2'

  $ echo commit3 > commit3
  $ git add .
  $ fn_git_commit -m 'commit3'

  $ echo commit4 > commit4
  $ git add .
  $ fn_git_commit -m 'commit4'

Pull one of them

  $ cd "$TESTTMP/hgrepo"

  $ hg log -r tip -T '{desc}\n'
  commit1

  $ hg external-sync "$TESTTMP/gitrepo" master 1
  importing up to 1 commits from $TESTTMP/gitrepo in master
  importing git objects into hg
  imported 1 commits
  $ hg log -r tip -T '{desc}\n'
  commit2

Pull the rest

  $ hg external-sync "$TESTTMP/gitrepo" master 3
  importing up to 3 commits from $TESTTMP/gitrepo in master
  importing git objects into hg
  imported 2 commits
  $ hg log -r tip -T '{desc}\n'
  commit4

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark master)

  $ ls
  commit1
  commit2
  commit3
  commit4

Nothing left ot pull

  $ hg external-sync "$TESTTMP/gitrepo" master 100
  importing up to 100 commits from $TESTTMP/gitrepo in master
  no changes found
  imported 0 commits
