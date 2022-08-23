#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True


  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ grep generaldelta master/.hg/requires
  generaldelta
  $ cd master
preferuncompressed = False so that we can make both generaldelta and non-generaldelta clones
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
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
  fetching tree '' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s
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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  fetching tree '' 126c4ddee02e922d5f05b4304b80e383a53a82e6
  1 trees fetched over 0.00s
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
  $ hg pull -u
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  fetching tree '' bc905f0fce7a7e7dfb60db06ddf9df54b3983840
  1 trees fetched over 0.00s
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

