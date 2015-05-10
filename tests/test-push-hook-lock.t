  $ hg init 1

  $ echo '[ui]' >> 1/.hg/hgrc
  $ echo 'timeout = 10' >> 1/.hg/hgrc

  $ echo foo > 1/foo
  $ hg --cwd 1 ci -A -m foo
  adding foo

  $ hg clone 1 2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg clone 2 3
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat <<EOF > $TESTTMP/debuglocks-pretxn-hook.sh
  > hg debuglocks
  > true
  > EOF
  $ echo '[hooks]' >> 2/.hg/hgrc
  $ echo "pretxnchangegroup.a = sh $TESTTMP/debuglocks-pretxn-hook.sh" >> 2/.hg/hgrc
  $ echo 'changegroup.push = hg push -qf ../1' >> 2/.hg/hgrc

  $ echo bar >> 3/foo
  $ hg --cwd 3 ci -m bar

  $ hg --cwd 3 push ../2 --config experimental.bundle2-exp=False
  pushing to ../2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  lock:  user *, process * (*s) (glob)
  wlock: free

  $ hg --cwd 1 --config extensions.strip= strip tip -q
  $ hg --cwd 2 --config extensions.strip= strip tip -q
  $ hg --cwd 3 push ../2 --config experimental.bundle2-exp=True
  pushing to ../2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  lock:  user *, process * (*s) (glob)
  wlock: user *, process * (*s) (glob)

