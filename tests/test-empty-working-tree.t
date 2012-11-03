Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ git commit --allow-empty -m empty >/dev/null 2>/dev/null || echo "git commit error"

  $ cd ..
  $ git init --bare gitrepo2
  Initialized empty Git repository in $TESTTMP/gitrepo2/

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log -r tip --template 'files: {files}\n'
  files: 

  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes

  $ cd ../gitrepo2
  $ git log --pretty=medium
  commit 678256865a8c85ae925bf834369264193c88f8de
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:00 2007 +0000
  
      empty

  $ cd ..
