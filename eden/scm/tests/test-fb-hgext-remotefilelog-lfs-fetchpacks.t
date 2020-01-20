#chg-compatible

  $ disable treemanifest

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=10B
  > EOF

  $ hg init master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF

  $ cd ..

# Push an LFS blob to the server.

  $ hgcloneshallow ssh://user@dummy/master push --noupdate
  streaming all changes
  0 files to transfer, 0 bytes of data
  transferred 0 bytes in * seconds (*/sec) (glob)
  no changes found
  $ cd push

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > backgroundrepack=True
  > getpackversion=1
  > [lfs]
  > url=file:$TESTTMP/dummy-remote/
  > EOF

  $ echo THIS-IS-LFS-FILE > x
  $ hg commit -qAm x-lfs
  $ hg push -q
  $ cd ..

  $ rm -rf push
  $ clearcache

  $ find $TESTTMP/dummy-remote | sort
  $TESTTMP/dummy-remote
  $TESTTMP/dummy-remote/80
  $TESTTMP/dummy-remote/80/2935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639

  $ hgcloneshallow ssh://user@dummy/master shallowv1 --noupdate
  streaming all changes
  3 files to transfer, * of data (glob)
  transferred 231 bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cd shallowv1

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > backgroundrepack=True
  > getpackversion=1
  > [lfs]
  > url=file:$TESTTMP/dummy-remote/
  > EOF

# With getpackv1, fetching the LFS blobs fails.

  $ hg update
  remote: abort: lfs.url needs to be configured
  abort: stream ended unexpectedly (got 0 bytes, expected 2)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)
  [255]

  $ cd ..

  $ rm -rf shallowv1
  $ clearcache

  $ hgcloneshallow ssh://user@dummy/master shallowv2 --noupdate
  streaming all changes
  3 files to transfer, * of data (glob)
  transferred 231 bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cd shallowv2

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > backgroundrepack=True
  > getpackversion=2
  > [lfs]
  > url=file:$TESTTMP/dummy-remote/
  > EOF

# With getpackv2, fetching the LFS blob succeed.

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)

  $ hg debugfilerev -v
  0d2948821b2b: x-lfs
   x: bin=0 lnk=0 flag=2000 size=17 copied='' chain=1ff4e6c9b276
    rawdata: 'version https://git-lfs.github.com/spec/v1\noid sha256:802935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639\nsize 17\nx-is-binary 0\n'

  $ hg show
  changeset:   0:0d2948821b2b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       x
  description:
  x-lfs
  
  
  diff -r 000000000000 -r 0d2948821b2b x
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-FILE
  
