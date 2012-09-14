# Fails for some reason, need to investigate
#   $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

bail early if the user is already running git-daemon
  $ ! (echo hi | nc localhost 9418 2>/dev/null) || exit 80

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
  >     git commit "$@" >/dev/null 2>/dev/null || echo "git commit error" | sed 's/, 0 deletions(-)//'
  >     count=`expr $count + 1`
  > }

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ echo alpha > alpha
  $ git add alpha
  $ commit -m 'add alpha'

  $ git checkout -b beta 2>&1 | sed s/\'/\"/g
  Switched to a new branch "beta"
  $ echo beta > beta
  $ git add beta
  $ commit -m 'add beta'

  $ git checkout master 2>&1 | sed s/\'/\"/g
  Switched to branch "master"
  $ echo gamma > gamma
  $ git add gamma
  $ commit -m 'add gamma'

clean merge
  $ git merge beta | sed "s/the '//;s/' strategy//" | sed 's/^Merge.*recursive.*$/Merge successful/' | sed 's/files/file/;s/insertions/insertion/;s/, 0 deletions.*//' | sed 's/|  */| /'
  Merge successful
   beta | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 beta

  $ cd ..
  $ mkdir gitrepo2
  $ cd gitrepo2
  $ git init --bare
  Initialized empty Git repository in $TESTTMP/gitrepo2/

dulwich does not presently support local git repos, workaround
  $ cd ..
  $ git daemon --base-path="$(pwd)"\
  >  --listen=localhost\
  >  --export-all \
  >  --pid-file="$DAEMON_PIDS" \
  >  --detach --reuseaddr \
  >  --enable=receive-pack

  $ hg clone git://localhost/gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo

  $ echo % clear the cache to be sure it is regenerated correctly
  % clear the cache to be sure it is regenerated correctly
  $ hg gclear
  clearing out the git cache data
  $ hg push git://localhost/gitrepo2
  pushing to git://localhost/gitrepo2
  exporting hg objects to git
  creating and sending data

  $ cd ..
  $ cd gitrepo2
  $ echo % git log in repo pushed from hg
  % git log in repo pushed from hg
  $ git log --pretty=medium master | sed 's/\.\.\.//g'
  commit 5806851511aaf3bfe813ae3a86c5027165fa9b96
  Merge: e5023f9 9497a4e
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      Merge branch 'beta'
  
  commit e5023f9e5cb24fdcec7b6c127cec45d8888e35a9
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      add gamma
  
  commit 9497a4ee62e16ee641860d7677cdb2589ea15554
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha
  $ git log --pretty=medium beta | sed 's/\.\.\.//g'
  commit 9497a4ee62e16ee641860d7677cdb2589ea15554
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha

  $ cd ..
