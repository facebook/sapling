Load commonly used test logic
  $ . "$TESTDIR/testutil"

set up a git repo with some commits, branches and a tag
  $ git init -q gitrepo
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'
  $ git tag t_alpha
  $ git checkout -qb beta
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'
  $ cd ..

clone a tag (ideally we'd want to pull it, but that seems broken for now)
#  $ hg init hgrepo
#  $ echo "[paths]" >> hgrepo/.hg/hgrc
#  $ echo "default=$TESTTMP/gitrepo" >> hgrepo/.hg/hgrc
#  $ hg -R hgrepo pull -r t_alpha
  $ hg clone -r t_alpha gitrepo hgrepo
  importing git objects into hg
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo log --graph
  @  changeset:   0:3442585be8a6
     bookmark:    master
     tag:         default/master
     tag:         t_alpha
     tag:         tip
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
pull a branch
  $ hg -R hgrepo pull -r beta
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg update' to get a working copy)
  $ hg -R hgrepo log --graph
  o  changeset:   1:7bcd915dc873
  |  bookmark:    beta
  |  tag:         default/beta
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  @  changeset:   0:3442585be8a6
     bookmark:    master
     tag:         default/master
     tag:         t_alpha
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
add another commit and tag to the git repo
  $ cd gitrepo
  $ git tag t_beta
  $ git checkout -q master
  $ echo gamma > gamma
  $ git add gamma
  $ fn_git_commit -m 'add gamma'
  $ cd ..

pull everything else
  $ hg -R hgrepo pull
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R hgrepo log --graph
  o  changeset:   2:37c124f2d0a0
  |  bookmark:    master
  |  tag:         default/master
  |  tag:         tip
  |  parent:      0:3442585be8a6
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add gamma
  |
  | o  changeset:   1:7bcd915dc873
  |/   bookmark:    beta
  |    tag:         default/beta
  |    tag:         t_beta
  |    user:        test <test@example.org>
  |    date:        Mon Jan 01 00:00:11 2007 +0000
  |    summary:     add beta
  |
  @  changeset:   0:3442585be8a6
     tag:         t_alpha
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
add a merge to the git repo
  $ cd gitrepo
  $ git merge beta | sed 's/|  */| /'
  Merge made by the 'recursive' strategy.
   beta | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 beta
  $ cd ..

pull the merge
  $ hg -R hgrepo pull
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg update' to get a working copy)
  $ hg -R hgrepo log --graph
  o    changeset:   3:b8668fddf56c
  |\   bookmark:    master
  | |  tag:         default/master
  | |  tag:         tip
  | |  parent:      2:37c124f2d0a0
  | |  parent:      1:7bcd915dc873
  | |  user:        test <test@example.org>
  | |  date:        Mon Jan 01 00:00:12 2007 +0000
  | |  summary:     Merge branch 'beta'
  | |
  | o  changeset:   2:37c124f2d0a0
  | |  parent:      0:3442585be8a6
  | |  user:        test <test@example.org>
  | |  date:        Mon Jan 01 00:00:12 2007 +0000
  | |  summary:     add gamma
  | |
  o |  changeset:   1:7bcd915dc873
  |/   bookmark:    beta
  |    tag:         default/beta
  |    tag:         t_beta
  |    user:        test <test@example.org>
  |    date:        Mon Jan 01 00:00:11 2007 +0000
  |    summary:     add beta
  |
  @  changeset:   0:3442585be8a6
     tag:         t_alpha
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
