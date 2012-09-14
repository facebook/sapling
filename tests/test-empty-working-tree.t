# Fails for some reason, need to investigate
#   $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

bail early if the user is already running git-daemon
  $ ! (echo hi | nc localhost 9418 2>/dev/null) || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH

  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/

  $ git commit --allow-empty -m empty >/dev/null 2>/dev/null || echo "git commit error"

  $ cd ..
  $ mkdir gitrepo2
  $ cd gitrepo2
  $ git init --bare
  Initialized empty Git repository in $TESTTMP/gitrepo2/

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
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log -r tip --template 'files: {files}\n'
  files: 

  $ hg gclear
  clearing out the git cache data
  $ hg push git://localhost/gitrepo2
  pushing to git://localhost/gitrepo2
  exporting hg objects to git
  creating and sending data

  $ cd ../gitrepo2
  $ git log --pretty=medium
  commit 678256865a8c85ae925bf834369264193c88f8de
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:00 2007 +0000
  
      empty

  $ cd ..
