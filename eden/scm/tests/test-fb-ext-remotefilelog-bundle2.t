#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ grep generaldelta master/.hg/requires
  generaldelta
  $ cd master
preferuncompressed = False so that we can make both generaldelta and non-generaldelta clones
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../sapling/ext/treemanifestserver.py
  > [remotefilelog]
  > server=True
  > [experimental]
  > bundle2-exp = True
  > [server]
  > preferuncompressed = False
  > [treemanifest]
  > server=True
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
  $ hg pull -u -d tip
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
push from generaldelta to generaldelta
  $ echo b > b
  $ hg commit -qAm b
  $ hg push
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
pull from generaldelta to non-generaldelta
  $ cd ../shallow-plain
  $ hg pull -u -d tip
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
push from non-generaldelta to generaldelta
  $ echo c > c
  $ hg commit -qAm c
  $ hg push
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes

