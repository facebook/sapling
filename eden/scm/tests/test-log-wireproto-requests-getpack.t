  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/helpers-wireprotologging.sh"

Create repo, enable capture requests
  $ newserver master
  $ capturewireprotologs

Clone repo, make changes
  $ clone master client1
  $ cd client1
  $ echo x > x
  $ hg commit -qAm x
  $ mkdir dir
  $ echo y > dir/y
  $ hg commit -qAm y
  $ hg push -r tip --to master --create -q
  remote: adding changesets (?)
  remote: adding manifests (?)
  remote: adding file changes (?)
  remote: added 2 changesets with 2 changes to 2 files (?)
  $ cd ..

Make getpackv1 request
  $ clone master v1 --noupdate
  $ cd v1
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > getpackversion=1
  > EOF

  $ clearcache

  $ hg up -q tip
  fetching tree '' 05bd2758dd7a25912490d0633b8975bf52bfab06, found via 79c51fb96423
  2 trees fetched over *s (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

Make getpackv2 request
  $ clone master v2 --noupdate
  $ cd v2
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > getpackversion=2
  > EOF

  $ clearcache

  $ hg up -q tip
  fetching tree '' 05bd2758dd7a25912490d0633b8975bf52bfab06, found via 79c51fb96423
  2 trees fetched over *s (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

Check logged requests
  $ grep 'getpack' $TESTTMP/loggedrequests
  wireproto_requests:  (args=[['dir/y', ['076f5e2225b3ff0400b98c92aa6cdf403ee24cca']], ['x', ['1406e74118627694268417491f018a4a883152f0']]], command=getpackv1, duration=*, reponame=unknown, responselen=*) (glob)
  wireproto_requests:  (args=[['dir/y', ['076f5e2225b3ff0400b98c92aa6cdf403ee24cca']], ['x', ['1406e74118627694268417491f018a4a883152f0']]], command=getpackv2, duration=*, reponame=unknown, responselen=*) (glob)
