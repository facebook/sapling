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

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ echo alpha > alpha
  $ git add alpha
  $ commit -m 'add alpha'
  $ echo beta > beta
  $ git add beta
  $ commit -m 'add beta'
  $ mkdir foo
  $ echo blah > foo/bar
  $ git add foo
  $ commit -m 'add foo'
  $ git rm alpha
  rm 'alpha'
  $ commit -m 'remove alpha'
  $ git rm foo/bar
  rm 'foo/bar'
  $ commit -m 'remove foo/bar'
  $ echo % final manifest in git is just beta
  % final manifest in git is just beta
  $ git ls-files
  beta

  $ cd ..
  $ mkdir gitrepo2
  $ cd gitrepo2
  $ git init --bare
  Initialized empty Git repository in $TESTTMP/gitrepo2/

  $ cd ..
  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --graph | grep -v ': *master'
  @  changeset:   4:ea41a3f0ed10
  |  tag:         default/master
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:14 2007 +0000
  |  summary:     remove foo/bar
  |
  o  changeset:   3:c84537f94bcc
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     remove alpha
  |
  o  changeset:   2:e25450e1354f
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add foo
  |
  o  changeset:   1:7bcd915dc873
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:3442585be8a6
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ echo
  
  $ echo % make sure alpha is not in this manifest
  % make sure alpha is not in this manifest
  $ hg manifest -r 3
  beta
  foo/bar

  $ echo
  
  $ echo % make sure that only beta is in the manifest
  % make sure that only beta is in the manifest
  $ hg manifest
  beta

  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes

  $ cd ../gitrepo2
  $ git log --pretty=medium
  commit b991de8952c482a7cd51162674ffff8474862218
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:14 2007 +0000
  
      remove foo/bar
  
  commit b0edaf0adac19392cf2867498b983bc5192b41dd
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      remove alpha
  
  commit f2d0d5bfa905e12dee728b509b96cf265bb6ee43
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      add foo
  
  commit 9497a4ee62e16ee641860d7677cdb2589ea15554
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha

  $ cd ..
