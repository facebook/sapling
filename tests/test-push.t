bail if the user does not have git command-line client
  $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH
  $ echo 'hgext.graphlog =' >> $HGRCPATH

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

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/

  $ echo alpha > alpha
  $ git add alpha
  $ commit -m "add alpha"
  $ git checkout -b not-master 2>&1 | sed s/\'/\"/g
  Switched to a new branch "not-master"

  $ cd ..
  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd hgrepo
  $ echo beta > beta
  $ hg add beta
  $ hgcommit -m 'add beta'


  $ echo gamma > gamma
  $ hg add gamma
  $ hgcommit -m 'add gamma'

  $ hg book -r 1 beta
  $ hg push -r beta
  pushing to $TESTTMP/gitrepo
  searching for changes

  $ cd ..

  $ echo % should have two different branches
  % should have two different branches
  $ cd gitrepo
  $ git branch -v
    beta       cffa0e8 add beta
    master     7eeab2e add alpha
  * not-master 7eeab2e add alpha

  $ echo % some more work on master from git
  % some more work on master from git
  $ git checkout master 2>&1 | sed s/\'/\"/g
  Switched to branch "master"
  $ echo delta > delta
  $ git add delta
  $ commit -m "add delta"
  $ git checkout not-master 2>&1 | sed s/\'/\"/g
  Switched to branch "not-master"

  $ cd ..

  $ cd hgrepo
  $ echo % this should fail
  % this should fail
  $ hg push -r master
  pushing to $TESTTMP/gitrepo
  searching for changes
  abort: refs/heads/master changed on the server, please pull and merge before pushing
  [255]

  $ echo % ... even with -f
  % ... even with -f
  $ hg push -fr master
  pushing to $TESTTMP/gitrepo
  searching for changes
  abort: refs/heads/master changed on the server, please pull and merge before pushing
  [255]

  $ hg pull 2>&1 | grep -v 'divergent bookmark'
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg update' to get a working copy)
TODO shouldn't need to do this since we're (in theory) pushing master explicitly,
which should not implicitly also push the not-master ref.
  $ hg book not-master -r default/not-master --force
  $ echo % master and default/master should be diferent
  % master and default/master should be diferent
  $ hg log -r master | grep -v ': *master'
  changeset:   2:72f56395749d
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  
  $ hg log -r default/master | grep -v 'master@default'
  changeset:   3:1436150b86c2
  tag:         default/master
  tag:         tip
  parent:      0:3442585be8a6
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:13 2007 +0000
  summary:     add delta
  

  $ echo % this should also fail
  % this should also fail
  $ hg push -r master
  pushing to $TESTTMP/gitrepo
  searching for changes
  abort: pushing refs/heads/master overwrites 72f56395749d
  [255]

  $ echo % ... but succeed with -f
  % ... but succeed with -f
  $ hg push -fr master
  pushing to $TESTTMP/gitrepo
  searching for changes

  $ echo % this should fail, no changes to push
  % this should fail, no changes to push
The exit code for this was broken in Mercurial (incorrectly returning 0) until
issue3228 was fixed in 2.1
  $ hg push -r master && false
  pushing to $TESTTMP/gitrepo
  searching for changes
  no changes found
  [1]

  $ cd ..

Push empty Hg repo to empty Git repo (issue #58)
Since there aren't any changes, exit code 1 is expected in modern Mercurial.
However, since it varies between supported Mercurial versions, we need to
force it to consistency for now. (see issue3228, fixed in Mercurial 2.1)
  $ hg init hgrepo2
  $ git init -q --bare gitrepo2
  $ hg -R hgrepo2 push gitrepo2 && false
  pushing to gitrepo2
  searching for changes
  no changes found
  [1]
