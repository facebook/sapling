Load commonly used test logic
  $ . "$TESTDIR/testutil"

bail if the user does not have git command-line client
  $ "$TESTDIR/hghave" git || exit 80

this test is busted on hg < 1.5. I'm not sure how to fix it.
  $ cat > tmp.py <<EOF
  > import sys
  > v = sys.stdin.read().strip()[:-1]
  > if v[1] == '.' and ((int(v[0]) == 1 and int(v[2]) > 4) or int(v[0]) > 1):
  >   sys.exit(0)
  > sys.exit(1)
  > EOF

  $ hg version | grep version | sed 's/.*(version //' | python tmp.py || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'

  $ git tag alpha

  $ git checkout -b beta 2>&1 | sed s/\'/\"/g
  Switched to a new branch "beta"
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'


  $ cd ..
  $ echo % clone a tag
  % clone a tag
  $ hg clone -r alpha gitrepo hgrepo-a | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo-a
  $ hg log --graph | egrep -v ': *(beta|master)'
  @  changeset:   0:3442585be8a6
     tag:         alpha
     tag:         default/master
     tag:         tip
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ cd ..
  $ echo % clone a branch
  % clone a branch
  $ hg clone -r beta gitrepo hgrepo-b | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo-b
  $ hg log --graph | egrep -v ': *(beta|master)'
  @  changeset:   1:7bcd915dc873
  |  tag:         default/beta
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:3442585be8a6
     tag:         alpha
     tag:         default/master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
  $ cd ..

  $ cd gitrepo
  $ echo beta line 2 >> beta
  $ git add beta
  $ fn_git_commit -m 'add to beta'

  $ cd ..
  $ cd hgrepo-b
  $ hg strip tip 2>&1 | grep -v saving | grep -v backup
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg pull -r beta
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  abort: you appear to have run strip - please run hg git-cleanup
  [255]
  $ hg git-cleanup
  git commit map cleaned
  $ echo % pull works after \'hg git-cleanup\'
  % pull works after 'hg git-cleanup'
"adding remote bookmark" message was added in Mercurial 2.3
  $ hg pull -r beta | grep -v "adding remote bookmark"
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg update' to get a working copy)
  $ hg log --graph | egrep -v 'bookmark: *(alpha|beta|master)'
  o  changeset:   2:611948b1ec6a
  |  tag:         default/beta
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add to beta
  |
  o  changeset:   1:7bcd915dc873
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  @  changeset:   0:3442585be8a6
     tag:         alpha
     tag:         default/master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ cd ..
