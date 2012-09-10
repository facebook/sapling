# Fails for some reason, need to investigate
#   $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

bail early if the user is already running git-daemon
  $ ! (echo hi | nc localhost 9418 2>/dev/null) || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH
  $ echo 'hgext.graphlog =' >> $HGRCPATH
  $ echo 'hgext.bookmarks =' >> $HGRCPATH

  $ hg init hgrepo1
  $ cd hgrepo1
  $ echo A > afile
  $ hg add afile
  $ hg ci -m "origin"

  $ echo B > afile
  $ hg ci -m "A->B"

  $ hg up -r0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo C > afile
  $ hg ci -m "A->C"
  created new head

  $ hg merge -r1 2>&1 | sed 's/-C ./-C/' | egrep -v '^merging afile$' | sed 's/incomplete.*/failed!/'
  warning: conflicts during merge.
  merging afile failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C' to abandon
resolve using second parent
  $ echo B > afile
  $ hg resolve -m afile
  $ hg ci -m "merge to B"

  $ hg log --graph --style compact | sed 's/\[.*\]//g'
  @    3:2,1   120385945d08   1970-01-01 00:00 +0000   test
  |\     merge to B
  | |
  | o  2:0   ea82b67264a1   1970-01-01 00:00 +0000   test
  | |    A->C
  | |
  o |  1   7205e83b5a3f   1970-01-01 00:00 +0000   test
  |/     A->B
  |
  o  0   5d1a6b64f9d0   1970-01-01 00:00 +0000   test
       origin
  

  $ cd ..

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init --bare | python -c "import sys; print sys.stdin.read().replace('$(dirname $(pwd))/', '')"
  Initialized empty Git repository in gitrepo/
  

dulwich does not presently support local git repos, workaround
  $ cd ..
  $ git daemon --base-path="$(pwd)"\
  >  --listen=localhost\
  >  --export-all\
  >  --pid-file="$DAEMON_PIDS" \
  >  --detach --reuseaddr \
  >  --enable=receive-pack

  $ cd hgrepo1
  $ hg bookmark -r tip master
  $ hg push -r master git://localhost/gitrepo
  pushing to git://localhost/gitrepo
  exporting hg objects to git
  creating and sending data
  $ cd ..

  $ hg clone git://localhost/gitrepo hgrepo2 | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo2
  $ echo % expect the same revision ids as above
  % expect the same revision ids as above
  $ hg log --graph --style compact | sed 's/\[.*\]//g'
  @    3:1,2   120385945d08   1970-01-01 00:00 +0000   test
  |\     merge to B
  | |
  | o  2:0   7205e83b5a3f   1970-01-01 00:00 +0000   test
  | |    A->B
  | |
  o |  1   ea82b67264a1   1970-01-01 00:00 +0000   test
  |/     A->C
  |
  o  0   5d1a6b64f9d0   1970-01-01 00:00 +0000   test
       origin
  

  $ cd ..
