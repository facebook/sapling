Init repo

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > sqldirstate=$(dirname $TESTDIR)/sqldirstate
  > EOF
  $ hg init repo
  $ cd repo
  $ mkdir a b
  $ echo a > a/a
  $ echo b > b/b
  $ echo c > c
  $ echo d > d
  $ echo x > x
  $ hg addremove -q
  $ hg st
  A a/a
  A b/b
  A c
  A d
  A x

Test automatic upgrade on pull

  $ cat <<EOF >> $HGRCPATH
  > [sqldirstate]
  > upgrade = True
  > EOF
  $ hg pull
  migrating your repo to sqldirstate which will make your hg commands faster
  done
  pulling from default
  abort: repository default not found!
  [255]
  $ ls .hg/dirstate*
  .hg/dirstate.sqlite3
  $ hg st
  A a/a
  A b/b
  A c
  A d
  A x
  $ hg pull
  pulling from default
  abort: repository default not found!
  [255]

Test conversions using debugcommands

  $ hg commit -m a
  $ hg st
  $ hg debugsqldirstate off
  $ hg st
  $ hg debugsqldirstate on
  $ hg st
  $ hg debugsqldirstate on
  abort: repo already has sqldirstate
  [255]

