Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'

  $ git tag alpha

  $ git checkout -b beta
  Switched to a new branch 'beta'
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'


  $ cd ..
clone a tag
  $ hg clone -r alpha gitrepo hgrepo-a | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo-a log --graph
  @  changeset:   0:69982ec78c6d
     bookmark:    master
     tag:         alpha
     tag:         default/master
     tag:         tip
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
clone a branch
  $ hg clone -r beta gitrepo hgrepo-b | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo-b log --graph
  @  changeset:   1:3bb02b6794dd
  |  bookmark:    beta
  |  tag:         default/beta
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:69982ec78c6d
     bookmark:    master
     tag:         alpha
     tag:         default/master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
  $ cd gitrepo
  $ echo beta line 2 >> beta
  $ git add beta
  $ fn_git_commit -m 'add to beta'

  $ cd ..
  $ cd hgrepo-b
  $ hg debugstrip tip 2>&1 | grep -v saving | grep -v backup
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg pull -r beta
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  abort: you appear to have run strip - please run hg git-cleanup
  [255]
  $ hg git-cleanup
  git commit map cleaned
pull works after 'hg git-cleanup'
"adding remote bookmark" message was added in Mercurial 2.3
  $ hg pull -r beta | grep -v "adding remote bookmark"
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  $ hg log --graph
  o  changeset:   2:3db9bf9073b5
  |  bookmark:    beta
  |  tag:         default/beta
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add to beta
  |
  o  changeset:   1:3bb02b6794dd
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  @  changeset:   0:69982ec78c6d
     bookmark:    master
     tag:         alpha
     tag:         default/master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ cd ..
