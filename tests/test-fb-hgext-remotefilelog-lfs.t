  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=10B
  > url=file:$TESTTMP/dummy-remote/
  > verify=existance
  > [diff]
  > git=1
  > EOF

# prepare a full repo with lfs metadata

  $ hg init master
  $ hg init lfs-upload-trigger
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo THIS-IS-LFS-FILE > x
  $ hg commit -qAm x-lfs
  $ hg mv x y
  $ hg commit -m y-lfs
  $ echo NOTLFS > y
  $ hg commit -m y-nonlfs
  $ hg mv y x
  $ hg commit -m x-nonlfs
  $ echo BECOME-LFS-AGAIN >> x
  $ hg commit -m x-lfs-again

  $ hg push -q ../lfs-upload-trigger

  $ cd ..

# shallow clone from full

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate
  streaming all changes
  2 files to transfer, 1.14 KB of data
  transferred 1.14 KB in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cd shallow

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ hg log -p -r ::tip -T '{rev}:{node} {desc}\n'
  0:0d2948821b2b3b6e58505696145f2215cea2b2cd x-lfs
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-FILE
  
  1:799bebfa53189a3db8424680f1a8f9806540e541 y-lfs
  diff --git a/x b/y
  rename from x
  rename to y
  
  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2 y-nonlfs
  diff --git a/y b/y
  --- a/y
  +++ b/y
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-FILE
  +NOTLFS
  
  3:c6cc0cd58884b847de39aa817ded71e6051caa9f x-nonlfs
  diff --git a/y b/x
  rename from y
  rename to x
  
  4:042535657086a5b08463b9210a8f46dc270e51f9 x-lfs-again
  diff --git a/x b/x
  --- a/x
  +++ b/x
  @@ -1,1 +1,2 @@
   NOTLFS
  +BECOME-LFS-AGAIN
  
  * files fetched over * (glob)

# lfs content could be read after repack

  $ hg repack

  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/8f2de7e341fbe688326386a45a3a7082d9f56871.histidx
  $TESTTMP/hgcache/master/packs/8f2de7e341fbe688326386a45a3a7082d9f56871.histpack
  $TESTTMP/hgcache/master/packs/fd280cbfab2f4047961d1ec5f7858e763ac985ab.dataidx
  $TESTTMP/hgcache/master/packs/fd280cbfab2f4047961d1ec5f7858e763ac985ab.datapack
  $TESTTMP/hgcache/master/packs/repacklock
  $TESTTMP/hgcache/repos

  $ cp -R . ../shallow2

  $ hg log -p -r ::tip -T '{rev}:{node} {desc}\n'
  0:0d2948821b2b3b6e58505696145f2215cea2b2cd x-lfs
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-FILE
  
  1:799bebfa53189a3db8424680f1a8f9806540e541 y-lfs
  diff --git a/x b/y
  rename from x
  rename to y
  
  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2 y-nonlfs
  diff --git a/y b/y
  --- a/y
  +++ b/y
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-FILE
  +NOTLFS
  
  3:c6cc0cd58884b847de39aa817ded71e6051caa9f x-nonlfs
  diff --git a/y b/x
  rename from y
  rename to x
  
  4:042535657086a5b08463b9210a8f46dc270e51f9 x-lfs-again
  diff --git a/x b/x
  --- a/x
  +++ b/x
  @@ -1,1 +1,2 @@
   NOTLFS
  +BECOME-LFS-AGAIN
  
# lfs working copy in shallow repo

  $ echo ADD-A-LINE >> x
  $ hg mv x y
  $ hg diff
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,2 +1,3 @@
   NOTLFS
   BECOME-LFS-AGAIN
  +ADD-A-LINE

  $ hg status -C
  A y
    x
  R x

  $ hg commit -m shallow.lfs.commit

  $ hg debugdata y f3e0509ec09891552c970794f82de8d6805701c7
  version https://git-lfs.github.com/spec/v1
  oid sha256:a2fcdb080e9838f6e1476a494c1d553e6ffefb68b0d146a06f34b535b5198442
  size 35
  x-hg-copy x
  x-hg-copyrev d33b2f7888d4f6f9112256d0f1c625af6d188fde
  x-is-binary 0

  $ hg debugfilerevision -r . y
  515a4dfd2e0c: shallow.lfs.commit
   y: bin=0 lnk=0 flag=2000 size=35 copied='x' chain=f3e0509ec098

  $ hg log -r . -T '{file_copies}\n'
  y (x)

# push lfs content to server

  $ hg push ../master
  pushing to ../master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

  $ cd ../master
  $ hg log -p -r tip -T '{rev}:{node} {desc}\n'
  5:515a4dfd2e0c4c963dcbf4bc48587b9747143598 shallow.lfs.commit
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,2 +1,3 @@
   NOTLFS
   BECOME-LFS-AGAIN
  +ADD-A-LINE
  
# pull lfs content from server and update

  $ cd ../shallow2
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 515a4dfd2e0c
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ hg update tip
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg log -p -r tip -T '{rev}:{node} {desc}\n'
  5:515a4dfd2e0c4c963dcbf4bc48587b9747143598 shallow.lfs.commit
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,2 +1,3 @@
   NOTLFS
   BECOME-LFS-AGAIN
  +ADD-A-LINE
  
# repack again

  $ cd ../shallow

  $ hg repack
  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/bf634767241b49b174b18732f92c6653ff966751.histidx
  $TESTTMP/hgcache/master/packs/bf634767241b49b174b18732f92c6653ff966751.histpack
  $TESTTMP/hgcache/master/packs/faa267575712c2ee0a4ff7e9c09bf75e10055c04.dataidx
  $TESTTMP/hgcache/master/packs/faa267575712c2ee0a4ff7e9c09bf75e10055c04.datapack
  $TESTTMP/hgcache/master/packs/repacklock
  $TESTTMP/hgcache/repos

  $ hg log -p -r ::tip -T '{rev}:{node} {desc}\n'
  0:0d2948821b2b3b6e58505696145f2215cea2b2cd x-lfs
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-FILE
  
  1:799bebfa53189a3db8424680f1a8f9806540e541 y-lfs
  diff --git a/x b/y
  rename from x
  rename to y
  
  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2 y-nonlfs
  diff --git a/y b/y
  --- a/y
  +++ b/y
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-FILE
  +NOTLFS
  
  3:c6cc0cd58884b847de39aa817ded71e6051caa9f x-nonlfs
  diff --git a/y b/x
  rename from y
  rename to x
  
  4:042535657086a5b08463b9210a8f46dc270e51f9 x-lfs-again
  diff --git a/x b/x
  --- a/x
  +++ b/x
  @@ -1,1 +1,2 @@
   NOTLFS
  +BECOME-LFS-AGAIN
  
  5:515a4dfd2e0c4c963dcbf4bc48587b9747143598 shallow.lfs.commit
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,2 +1,3 @@
   NOTLFS
   BECOME-LFS-AGAIN
  +ADD-A-LINE
  
# bundle should not include LFS blobs

  $ cat > noise.py <<EOF
  > import os
  > import sys
  > # random content so compression is ineffective
  > length = int(sys.argv[1])
  > sys.stdout.write(os.urandom(length))
  > sys.stdout.flush()
  > EOF
  $ hg bookmark -i base
  $ cp -R . ../shallow3
  $ $PYTHON noise.py 20000000 >> y
  $ hg commit -m 'make y 20MB' y
  $ $PYTHON noise.py  1000000 >> y
  $ hg commit -m 'make y 1MB'
  $ hg bundle -r '(base::)-base' --base base test-bundle
  2 changesets found
  $ $PYTHON <<EOF
  > import os
  > size = os.stat('test-bundle').st_size
  > if size <= 10000:
  >     print('size is less than 10 KB - expected')
  > else:
  >     print('unexpected size: %s' % size)
  > EOF
  size is less than 10 KB - expected

# Applying the bundle should work

  $ cd ../shallow3
  $ hg unbundle ../shallow/test-bundle
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  new changesets * (glob)

  $ hg update tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# LFS fast path about binary diff works

  $ cd ../shallow
  $ hg pull -q

  $ cd ../shallow2
  $ hg up -C tip -q
  $ $PYTHON << EOF
  > with open('a.bin', 'wb') as f:
  >     f.write(b'\x00\x01\x02\x00' * 10)
  > EOF

  $ hg commit -m binary -A a.bin
  $ for i in 1 2; do
  >    echo $i >> a.bin
  >    hg commit -m $i a.bin
  > done

  $ chmod +x a.bin
  $ hg commit -m 'mode change' a.bin

  $ for i in 3 4; do
  >    echo $i >> a.bin
  >    hg commit -m $i a.bin
  > done

  $ hg push ../master
  pushing to ../master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 5 changes to 1 files

  $ cd ../shallow
  $ hg pull -q
  5 files fetched over 5 fetches - (5 misses, 0.00% hit ratio) over *s (glob)
  $ hg log --removed a.bin --config diff.nobinary=1 --git -p -T '{desc}\n' -r '::tip' --config lfs.url=null://
  binary
  diff --git a/a.bin b/a.bin
  new file mode 100644
  Binary file a.bin has changed
  
  1
  diff --git a/a.bin b/a.bin
  Binary file a.bin has changed
  
  2
  diff --git a/a.bin b/a.bin
  Binary file a.bin has changed
  
  mode change
  diff --git a/a.bin b/a.bin
  old mode 100644
  new mode 100755
  
  3
  diff --git a/a.bin b/a.bin
  Binary file a.bin has changed
  
  4
  diff --git a/a.bin b/a.bin
  Binary file a.bin has changed
  

