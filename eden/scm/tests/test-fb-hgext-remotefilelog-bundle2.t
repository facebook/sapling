#chg-compatible

  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ grep generaldelta master/.hg/requires
  generaldelta
  $ cd master
preferuncompressed = False so that we can make both generaldelta and non-generaldelta clones
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > [experimental]
  > bundle2-exp = True
  > [server]
  > preferuncompressed = False
  > EOF
  $ echo x > x
  $ hg commit -qAm x

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow-generaldelta -q --pull --config experimental.bundle2-exp=True
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  $ grep generaldelta shallow-generaldelta/.hg/requires
  generaldelta
  $ hgcloneshallow ssh://user@dummy/master shallow-plain -q --pull --config format.usegeneraldelta=False --config format.generaldelta=False --config experimental.bundle2-exp=True
  $ grep generaldelta shallow-plain/.hg/requires
  [1]

  $ cd master
  $ echo a > a
  $ hg commit -qAm a

pull from generaldelta to generaldelta
  $ cd ../shallow-generaldelta
  $ hg pull -u
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
push from generaldelta to generaldelta
  $ echo b > b
  $ hg commit -qAm b
  $ hg push
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
pull from generaldelta to non-generaldelta
  $ cd ../shallow-plain
  $ hg pull -u
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
push from non-generaldelta to generaldelta
  $ echo c > c
  $ hg commit -qAm c
  $ hg push
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

