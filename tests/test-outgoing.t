bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

bail early if the user is already running git-daemon
  $ ! (echo hi | nc localhost 9418 2>/dev/null) || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH
  $ echo 'hgext.graphlog =' >> $HGRCPATH
  $ echo 'hgext.bookmarks =' >> $HGRCPATH

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
  $ git init | python -c "import sys; print sys.stdin.read().replace('$(dirname $(pwd))/', '')"
  Initialized empty Git repository in gitrepo/.git/
  

  $ echo alpha > alpha
  $ git add alpha
  $ commit -m "add alpha"

dulwich does not presently support local git repos, workaround
  $ cd ..
  $ git daemon --base-path="$(pwd)"\
  >  --listen=localhost\
  >  --export-all\
  >  --pid-file="$DAEMON_PIDS" \
  >  --detach --reuseaddr \
  >  --enable=receive-pack

  $ hg clone git://localhost/gitrepo hgrepo | grep -v '^updating'
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

  $ hg outgoing | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with git://localhost/gitrepo
  exporting hg objects to git
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  changeset:   2:72f56395749d
  tag:         master
  tag:         tip
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  
  $ hg outgoing -r beta | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with git://localhost/gitrepo
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  $ hg outgoing -r master | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with git://localhost/gitrepo
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  changeset:   2:72f56395749d
  tag:         master
  tag:         tip
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  

  $ cd ..

  $ echo % some more work on master from git
  % some more work on master from git
  $ cd gitrepo

  $ git checkout master 2>&1 | sed s/\'/\"/g
  Already on "master"
  $ echo delta > delta
  $ git add delta
  $ commit -m "add delta"

  $ cd ..

  $ cd hgrepo
  $ echo % this will fail # maybe we should try to make it work
  % this will fail
  $ hg outgoing
  comparing with git://localhost/gitrepo
  abort: refs/heads/master changed on the server, please pull and merge before pushing
  [255]
  $ echo % let\'s pull and try again
  % let's pull and try again
  $ hg pull
  pulling from git://localhost/gitrepo
  importing git objects into hg
  (run 'hg update' to get a working copy)
  $ hg outgoing | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with git://localhost/gitrepo
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  changeset:   2:72f56395749d
  tag:         master
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  
  $ hg outgoing -r beta | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with git://localhost/gitrepo
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  $ hg outgoing -r master | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with git://localhost/gitrepo
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  changeset:   2:72f56395749d
  tag:         master
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  


  $ cd ..
