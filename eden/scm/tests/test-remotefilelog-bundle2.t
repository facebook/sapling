#require no-eden

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ grep generaldelta master/.sl/requires
  generaldelta
  $ cd master
preferuncompressed = False so that we can make both generaldelta and non-generaldelta clones
  $ cat >> .sl/config <<EOF
  > [remotefilelog]
  > server=True
  > [experimental]
  > bundle2-exp = True
  > [server]
  > preferuncompressed = False
  > EOF
  $ echo x > x
  $ sl commit -qAm x
  $ sl book master

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow-generaldelta -q --config experimental.bundle2-exp=True
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  $ grep generaldelta shallow-generaldelta/.sl/requires
  generaldelta
  $ hgcloneshallow ssh://user@dummy/master shallow-plain -q --config format.usegeneraldelta=False --config format.generaldelta=False --config experimental.bundle2-exp=True
  $ grep generaldelta shallow-plain/.sl/requires
  [1]

  $ cd master
  $ echo a > a
  $ sl commit -qAm a

pull from generaldelta to generaldelta
  $ cd ../shallow-generaldelta
  $ sl pull -q -u -d master
push from generaldelta to generaldelta
  $ echo b > b
  $ sl commit -qAm b
  $ sl push --allow-anon
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
pull from generaldelta to non-generaldelta
  $ cd ../shallow-plain
  $ sl pull -q -u -d master
push from non-generaldelta to generaldelta
  $ echo c > c
  $ sl commit -qAm c
  $ sl push --allow-anon
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
