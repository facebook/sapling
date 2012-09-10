# Fails for some reason, need to investigate
#   $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

bail early if the user is already running git-daemon
  $ ! (echo hi | nc localhost 9418 2>/dev/null) || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH
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

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init | python -c "import sys; print sys.stdin.read().replace('$(dirname $(pwd))/', '')"
  Initialized empty Git repository in gitrepo/.git/
  

  $ mkdir d1
  $ echo a > d1/f1
  $ echo b > d1/f2
  $ git add d1/f1 d1/f2
  $ commit -m initial

  $ mkdir d2
  $ git mv d1/f2 d2/f2
  $ commit -m 'rename'

  $ rm -r d1
  $ echo c > d1
  $ git add d1
  $ commit -m 'replace a dir with a file'


  $ cd ..
  $ mkdir gitrepo2
  $ cd gitrepo2
  $ git init --bare | python -c "import sys; print sys.stdin.read().replace('$(dirname $(pwd))/', '')"
  Initialized empty Git repository in gitrepo2/
  

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
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --template 'adds: {file_adds}\ndels: {file_dels}\n'
  adds: d1
  dels: d1/f1
  adds: d2/f2
  dels: d1/f2
  adds: d1/f1 d1/f2
  dels: 

  $ hg gclear
  clearing out the git cache data
  $ hg push git://localhost/gitrepo2
  pushing to git://localhost/gitrepo2
  exporting hg objects to git
  creating and sending data

  $ cd ../gitrepo2
  $ git log --pretty=medium
  commit 6e0dbd8cd92ed4823c69cb48d8a2b81f904e6e69
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      replace a dir with a file
  
  commit a1874d5cd0b1549ed729e36f0da4a93ed36259ee
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      rename
  
  commit 102c17a5deda49db3f10ec5573f9378867098b7c
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      initial

  $ cd ..
