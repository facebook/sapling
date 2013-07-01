bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH
  $ echo 'hgext.graphlog =' >> $HGRCPATH
  $ echo "[git]" >> $HGRCPATH
  $ echo "branch_bookmark_suffix=_bookmark" >> $HGRCPATH

  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE

  $ count=10
  $ commit()
  > {
  >     GIT_AUTHOR_DATE="2007-01-01 00:00:$count +0000"
  >     GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"
  >     git commit "$@" >/dev/null 2>/dev/null || echo "git commit error"
  >     count=`expr $count + 1`
  > }
  $ hgcommit()
  > {
  >     HGDATE="2007-01-01 00:00:$count +0000"
  >     hg commit -d "$HGDATE" "$@" >/dev/null 2>/dev/null || echo "hg commit error"
  >     count=`expr $count + 1`
  > }

  $ git config --global push.default matching
  $ git init --bare gitrepo1
  Initialized empty Git repository in $TESTTMP/gitrepo1/

  $ hg init hgrepo
  $ cd hgrepo
  $ hg branch -q branch1
  $ hg bookmark branch1_bookmark
  $ echo f1 > f1
  $ hg add f1
  $ hgcommit -m "add f1"
  $ hg branch -q branch2
  $ hg bookmark branch2_bookmark
  $ echo f2 > f2
  $ hg add f2
  $ hgcommit -m "add f2"
  $ hg log --graph
  @  changeset:   1:600de9b6d498
  |  branch:      branch2
  |  bookmark:    branch2_bookmark
  |  tag:         tip
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add f2
  |
  o  changeset:   0:40a840c1f8ae
     branch:      branch1
     bookmark:    branch1_bookmark
     user:        test
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add f1
  


  $ hg push ../gitrepo1
  pushing to ../gitrepo1
  searching for changes
  adding objects
  added 2 commits with 2 trees and 2 blobs

  $ cd ..

  $ cd gitrepo1
  $ git symbolic-ref HEAD refs/heads/branch1
  $ git branch
  * branch1
    branch2
  $ cd ..

  $ git clone gitrepo1 gitrepo2
  Cloning into 'gitrepo2'...
  done.
  $ cd gitrepo2
  $ git checkout branch1
  Already on 'branch1'
  $ echo g1 >> f1
  $ git add f1
  $ commit -m "append f1"
  $ git checkout branch2
  Switched to a new branch 'branch2'
  Branch branch2 set up to track remote branch branch2 from origin.
  $ echo g2 >> f2
  $ git add f2
  $ commit -m "append f2"
  $ git push origin
  To $TESTTMP/gitrepo1
     bbfe79a..d8aef79  branch1 -> branch1
     288e92b..f8f8de5  branch2 -> branch2
  $ cd ..

  $ cd hgrepo
  $ hg pull ../gitrepo1
  pulling from ../gitrepo1
  importing git objects into hg
  (run 'hg heads' to see heads)
  $ hg log --graph
  o  changeset:   3:0a696ec0f478
  |  bookmark:    branch2_bookmark
  |  tag:         default/branch2
  |  tag:         tip
  |  parent:      1:600de9b6d498
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     append f2
  |
  | o  changeset:   2:49db35e15e81
  | |  bookmark:    branch1_bookmark
  | |  tag:         default/branch1
  | |  parent:      0:40a840c1f8ae
  | |  user:        test <test@example.org>
  | |  date:        Mon Jan 01 00:00:12 2007 +0000
  | |  summary:     append f1
  | |
  @ |  changeset:   1:600de9b6d498
  |/   branch:      branch2
  |    user:        test
  |    date:        Mon Jan 01 00:00:11 2007 +0000
  |    summary:     add f2
  |
  o  changeset:   0:40a840c1f8ae
     branch:      branch1
     user:        test
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add f1
  


  $ cd ..
