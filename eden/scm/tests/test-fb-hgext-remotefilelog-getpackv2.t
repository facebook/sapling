#chg-compatible

  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ clone master client1
  $ cd client1
  $ echo x > x
  $ hg commit -qAm x
  $ mkdir dir
  $ echo y > dir/y
  $ hg commit -qAm y
  $ hg push -r tip --to master --create
  pushing rev 79c51fb96423 to destination ssh://user@dummy/master bookmark master
  searching for changes
  remote: adding changesets (?)
  remote: adding manifests (?)
  remote: adding file changes (?)
  remote: added 2 changesets with 2 changes to 2 files (?)
  exporting bookmark master
  $ cd ..

Shallow clone from full

  $ clone master shallow --noupdate
  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > getpackversion=2
  > EOF

  $ hg up -q tip
  fetching tree '' 05bd2758dd7a25912490d0633b8975bf52bfab06, found via 79c51fb96423
  2 trees fetched over *s (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg debugfilerev -v
  79c51fb96423: y
   dir/y: bin=0 lnk=0 flag=0 size=2 copied='' chain=076f5e2225b3
    rawdata: 'y\n'
