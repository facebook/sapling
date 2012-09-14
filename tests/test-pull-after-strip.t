# Fails for some reason, need to investigate
#   $ "$TESTDIR/hghave" git || exit 80

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

bail early if the user is already running git-daemon
  $ ! (echo hi | nc localhost 9418 2>/dev/null) || exit 80

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > mq=
  > EOF
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH

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

  $ git tag alpha

  $ git checkout -b beta 2>&1 | sed s/\'/\"/g
  Switched to a new branch "beta"
  $ echo beta > beta
  $ git add beta
  $ commit -m 'add beta'


  $ cd ..
  $ git daemon --base-path="$(pwd)"\
  >  --listen=localhost\
  >  --export-all\
  >  --pid-file="$DAEMON_PIDS" \
  >  --detach --reuseaddr

  $ echo % clone a tag
  % clone a tag
  $ hg clone -r alpha git://localhost/gitrepo hgrepo-a | grep -v '^updating'
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
  $ hg clone -r beta git://localhost/gitrepo hgrepo-b | grep -v '^updating'
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
  $ commit -m 'add to beta'

  $ cd ..
  $ cd hgrepo-b
  $ hg strip tip 2>&1 | grep -v saving | grep -v backup
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg pull -r beta
  pulling from git://localhost/gitrepo
  importing git objects into hg
  abort: you appear to have run strip - please run hg git-cleanup
  [255]
  $ hg git-cleanup
  git commit map cleaned
  $ echo % pull works after \'hg git-cleanup\'
  % pull works after 'hg git-cleanup'
  $ hg pull -r beta
  pulling from git://localhost/gitrepo
  importing git objects into hg
  (run 'hg update' to get a working copy)
  $ hg log --graph | egrep -v ': *(beta|master)'
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
