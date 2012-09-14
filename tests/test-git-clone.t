# Fails for some reason, need to investigate
#   $ "$TESTDIR/hghave" git || exit 80

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

  $ cd ..

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --graph  | grep -v ': *master'
  @  changeset:   1:7bcd915dc873
  |  tag:         default/master
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:3442585be8a6
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ echo % we should have some bookmarks
  % we should have some bookmarks
  $ hg book
   * master                    1:7bcd915dc873
