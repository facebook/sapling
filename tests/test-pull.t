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
  $ git checkout -qb delta master
  $ echo delta > delta
  $ git add delta
  $ fn_git_commit -m 'add delta'
  $ cd ..

pull a tag
  $ hg init hgrepo
  $ echo "[paths]" >> hgrepo/.hg/hgrc
  $ echo "default=$TESTTMP/gitrepo" >> hgrepo/.hg/hgrc
  $ hg -R hgrepo pull -r t_alpha
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg update' to get a working copy)
  $ hg -R hgrepo update t_alpha
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo log --graph
  @  changeset:   0:ff7a2f2d8d70
     bookmark:    master
     tag:         default/master
     tag:         t_alpha
     tag:         tip
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
no-op pull
  $ hg -R hgrepo pull -r t_alpha
  pulling from $TESTTMP/gitrepo
  no changes found

no-op pull with added bookmark
  $ cd gitrepo
  $ git checkout -qb epsilon t_alpha
  $ cd ..
  $ hg -R hgrepo pull -r epsilon
  pulling from $TESTTMP/gitrepo
  no changes found

pull a branch
  $ hg -R hgrepo pull -r beta
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg update' to get a working copy)
  $ hg -R hgrepo log --graph
  o  changeset:   1:7fe02317c63d
  |  bookmark:    beta
  |  tag:         default/beta
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  @  changeset:   0:ff7a2f2d8d70
     bookmark:    epsilon
     bookmark:    master
     tag:         default/epsilon
     tag:         default/master
     tag:         t_alpha
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
add another commit and tag to the git repo
  $ cd gitrepo
  $ git checkout -q beta
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
  o  changeset:   3:6f898ad1f3e1
  |  bookmark:    master
  |  tag:         default/master
  |  tag:         tip
  |  parent:      0:ff7a2f2d8d70
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     add gamma
  |
  | o  changeset:   2:678ebee93e38
  |/   bookmark:    delta
  |    tag:         default/delta
  |    parent:      0:ff7a2f2d8d70
  |    user:        test <test@example.org>
  |    date:        Mon Jan 01 00:00:12 2007 +0000
  |    summary:     add delta
  |
  | o  changeset:   1:7fe02317c63d
  |/   bookmark:    beta
  |    tag:         default/beta
  |    tag:         t_beta
  |    user:        test <test@example.org>
  |    date:        Mon Jan 01 00:00:11 2007 +0000
  |    summary:     add beta
  |
  @  changeset:   0:ff7a2f2d8d70
     bookmark:    epsilon
     tag:         default/epsilon
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
  o    changeset:   4:a02330f767a4
  |\   bookmark:    master
  | |  tag:         default/master
  | |  tag:         tip
  | |  parent:      3:6f898ad1f3e1
  | |  parent:      1:7fe02317c63d
  | |  user:        test <test@example.org>
  | |  date:        Mon Jan 01 00:00:13 2007 +0000
  | |  summary:     Merge branch 'beta'
  | |
  | o  changeset:   3:6f898ad1f3e1
  | |  parent:      0:ff7a2f2d8d70
  | |  user:        test <test@example.org>
  | |  date:        Mon Jan 01 00:00:13 2007 +0000
  | |  summary:     add gamma
  | |
  | | o  changeset:   2:678ebee93e38
  | |/   bookmark:    delta
  | |    tag:         default/delta
  | |    parent:      0:ff7a2f2d8d70
  | |    user:        test <test@example.org>
  | |    date:        Mon Jan 01 00:00:12 2007 +0000
  | |    summary:     add delta
  | |
  o |  changeset:   1:7fe02317c63d
  |/   bookmark:    beta
  |    tag:         default/beta
  |    tag:         t_beta
  |    user:        test <test@example.org>
  |    date:        Mon Jan 01 00:00:11 2007 +0000
  |    summary:     add beta
  |
  @  changeset:   0:ff7a2f2d8d70
     bookmark:    epsilon
     tag:         default/epsilon
     tag:         t_alpha
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
pull with wildcards
  $ cd gitrepo
  $ git checkout -qb releases/v1 master
  $ echo zeta > zeta
  $ git add zeta
  $ fn_git_commit -m 'add zeta'
  $ git checkout -qb releases/v2 master
  $ echo eta > eta
  $ git add eta
  $ fn_git_commit -m 'add eta'
  $ git checkout -qb notreleases/v1 master
  $ echo theta > theta
  $ git add theta
  $ fn_git_commit -m 'add theta'

ensure that releases/v1 and releases/v2 are pulled but not notreleases/v1
  $ cd ..
  $ hg -R hgrepo pull -r 'releases/*'
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R hgrepo log --graph
  o  changeset:   6:a3f95e150b0a
  |  bookmark:    releases/v2
  |  tag:         default/releases/v2
  |  tag:         tip
  |  parent:      4:a02330f767a4
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:15 2007 +0000
  |  summary:     add eta
  |
  | o  changeset:   5:218b2d0660d3
  |/   bookmark:    releases/v1
  |    tag:         default/releases/v1
  |    user:        test <test@example.org>
  |    date:        Mon Jan 01 00:00:14 2007 +0000
  |    summary:     add zeta
  |
  o    changeset:   4:a02330f767a4
  |\   bookmark:    master
  | |  tag:         default/master
  | |  parent:      3:6f898ad1f3e1
  | |  parent:      1:7fe02317c63d
  | |  user:        test <test@example.org>
  | |  date:        Mon Jan 01 00:00:13 2007 +0000
  | |  summary:     Merge branch 'beta'
  | |
  | o  changeset:   3:6f898ad1f3e1
  | |  parent:      0:ff7a2f2d8d70
  | |  user:        test <test@example.org>
  | |  date:        Mon Jan 01 00:00:13 2007 +0000
  | |  summary:     add gamma
  | |
  | | o  changeset:   2:678ebee93e38
  | |/   bookmark:    delta
  | |    tag:         default/delta
  | |    parent:      0:ff7a2f2d8d70
  | |    user:        test <test@example.org>
  | |    date:        Mon Jan 01 00:00:12 2007 +0000
  | |    summary:     add delta
  | |
  o |  changeset:   1:7fe02317c63d
  |/   bookmark:    beta
  |    tag:         default/beta
  |    tag:         t_beta
  |    user:        test <test@example.org>
  |    date:        Mon Jan 01 00:00:11 2007 +0000
  |    summary:     add beta
  |
  @  changeset:   0:ff7a2f2d8d70
     bookmark:    epsilon
     tag:         default/epsilon
     tag:         t_alpha
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

add old and new commits to the git repo -- make sure we're using the commit date
and not the author date
  $ cat >> $HGRCPATH <<EOF
  > [git]
  > mindate = 2014-01-02 00:00:00 +0000
  > EOF
  $ cd gitrepo
  $ git checkout -q master
  $ echo oldcommit > oldcommit
  $ git add oldcommit
  $ GIT_AUTHOR_DATE="2014-03-01 00:00:00 +0000" \
  > GIT_COMMITTER_DATE="2009-01-01 00:00:00 +0000" \
  > git commit -m oldcommit > /dev/null || echo "git commit error"
also add an annotated tag
  $ git checkout -q master^
  $ echo oldtag > oldtag
  $ git add oldtag
  $ GIT_AUTHOR_DATE="2014-03-01 00:00:00 +0000" \
  > GIT_COMMITTER_DATE="2009-01-01 00:00:00 +0000" \
  > git commit -m oldtag > /dev/null || echo "git commit error"
  $ GIT_COMMITTER_DATE="2009-02-01 00:00:00 +0000" \
  > git tag -a -m 'tagging oldtag' oldtag
  $ cd ..
  $ hg -R hgrepo pull
  pulling from $TESTTMP/gitrepo
  no changes found
  $ hg -R hgrepo log -r master
  changeset:   4:a02330f767a4
  bookmark:    master
  tag:         default/master
  parent:      3:6f898ad1f3e1
  parent:      1:7fe02317c63d
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:13 2007 +0000
  summary:     Merge branch 'beta'
  

  $ cd gitrepo
  $ git checkout -q master
  $ echo newcommit > newcommit
  $ git add newcommit
  $ GIT_AUTHOR_DATE="2014-01-01 00:00:00 +0000" \
  > GIT_COMMITTER_DATE="2014-01-02 00:00:00 +0000" \
  > git commit -m newcommit > /dev/null || echo "git commit error"
  $ git checkout -q refs/tags/oldtag
  $ GIT_COMMITTER_DATE="2014-01-02 00:00:00 +0000" \
  > git tag -a -m 'tagging newtag' newtag
  $ cd ..
  $ hg -R hgrepo pull
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R hgrepo heads
  changeset:   9:e103a73f33be
  bookmark:    master
  tag:         default/master
  tag:         tip
  user:        test <test@example.org>
  date:        Wed Jan 01 00:00:00 2014 +0000
  summary:     newcommit
  
  changeset:   7:49713da8f665
  tag:         newtag
  tag:         oldtag
  parent:      4:a02330f767a4
  user:        test <test@example.org>
  date:        Sat Mar 01 00:00:00 2014 +0000
  summary:     oldtag
  
  changeset:   6:a3f95e150b0a
  bookmark:    releases/v2
  tag:         default/releases/v2
  parent:      4:a02330f767a4
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:15 2007 +0000
  summary:     add eta
  
  changeset:   5:218b2d0660d3
  bookmark:    releases/v1
  tag:         default/releases/v1
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:14 2007 +0000
  summary:     add zeta
  
  changeset:   2:678ebee93e38
  bookmark:    delta
  tag:         default/delta
  parent:      0:ff7a2f2d8d70
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add delta
  
