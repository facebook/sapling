#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest
  $ enable remotenames
  $ setconfig remotenames.selectivepull=1 remotefilelog.lfs=True

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
  $ hg bookmark master -R lfs-upload-trigger
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
  $ hg bookmark master

  $ hg push -q ../lfs-upload-trigger --to master

  $ cd ..

# shallow clone from full

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate
  fetching changelog
  3 files to transfer, * of data (glob)
  transferred 1.14 KB in * seconds (*/sec) (glob)
  fetching selected remote bookmarks
  $ cd shallow

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg log -p -r ::tip -T '{node} {desc}\n'
  * files fetched over * (glob) (?)
  0d2948821b2b3b6e58505696145f2215cea2b2cd x-lfs
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-FILE
  
  799bebfa53189a3db8424680f1a8f9806540e541 y-lfs
  diff --git a/x b/y
  rename from x
  rename to y
  
  f3dec7f3610207dbf222ec2d7b68df16a5fde0f2 y-nonlfs
  diff --git a/y b/y
  --- a/y
  +++ b/y
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-FILE
  +NOTLFS
  
  c6cc0cd58884b847de39aa817ded71e6051caa9f x-nonlfs
  diff --git a/y b/x
  rename from y
  rename to x
  
  042535657086a5b08463b9210a8f46dc270e51f9 x-lfs-again
  diff --git a/x b/x
  --- a/x
  +++ b/x
  @@ -1,1 +1,2 @@
   NOTLFS
  +BECOME-LFS-AGAIN
  
  $ cp -R . ../shallow2

  $ hg log -p -r ::tip -T '{node} {desc}\n'
  0d2948821b2b3b6e58505696145f2215cea2b2cd x-lfs
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-FILE
  
  799bebfa53189a3db8424680f1a8f9806540e541 y-lfs
  diff --git a/x b/y
  rename from x
  rename to y
  
  f3dec7f3610207dbf222ec2d7b68df16a5fde0f2 y-nonlfs
  diff --git a/y b/y
  --- a/y
  +++ b/y
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-FILE
  +NOTLFS
  
  c6cc0cd58884b847de39aa817ded71e6051caa9f x-nonlfs
  diff --git a/y b/x
  rename from y
  rename to x
  
  042535657086a5b08463b9210a8f46dc270e51f9 x-lfs-again
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
  
  copy: x
  copyrev: d33b2f7888d4f6f9112256d0f1c625af6d188fde
  
  NOTLFS
  BECOME-LFS-AGAIN
  ADD-A-LINE

  $ hg debugfilerevision -r . y
  515a4dfd2e0c: shallow.lfs.commit
   y: bin=0 lnk=0 flag=0 size=35 copied='x' chain=f3e0509ec098

  $ hg log -r . -T '{file_copies}\n'
  y (x)

# push lfs content to server

  $ hg push ../master --to master
  pushing rev 515a4dfd2e0c to destination ../master bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master

  $ cd ../master
  $ hg log -p -r tip -T '{node} {desc}\n'
  515a4dfd2e0c4c963dcbf4bc48587b9747143598 shallow.lfs.commit
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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg update tip
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg log -p -r tip -T '{node} {desc}\n'
  515a4dfd2e0c4c963dcbf4bc48587b9747143598 shallow.lfs.commit
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,2 +1,3 @@
   NOTLFS
   BECOME-LFS-AGAIN
  +ADD-A-LINE
  
  $ cd ../shallow

  $ hg log -p -r ::tip -T '{node} {desc}\n'
  0d2948821b2b3b6e58505696145f2215cea2b2cd x-lfs
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-FILE
  
  799bebfa53189a3db8424680f1a8f9806540e541 y-lfs
  diff --git a/x b/y
  rename from x
  rename to y
  
  f3dec7f3610207dbf222ec2d7b68df16a5fde0f2 y-nonlfs
  diff --git a/y b/y
  --- a/y
  +++ b/y
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-FILE
  +NOTLFS
  
  c6cc0cd58884b847de39aa817ded71e6051caa9f x-nonlfs
  diff --git a/y b/x
  rename from y
  rename to x
  
  042535657086a5b08463b9210a8f46dc270e51f9 x-lfs-again
  diff --git a/x b/x
  --- a/x
  +++ b/x
  @@ -1,1 +1,2 @@
   NOTLFS
  +BECOME-LFS-AGAIN
  
  515a4dfd2e0c4c963dcbf4bc48587b9747143598 shallow.lfs.commit
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
  > # PY3-compat
  > if sys.version_info[0] >= 3:
  >     stdout = sys.stdout.buffer
  > else:
  >     stdout = sys.stdout
  > # random content so compression is ineffective
  > length = int(sys.argv[1])
  > stdout.write(os.urandom(length))
  > stdout.flush()
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

  $ hg push ../master --to master
  pushing rev d6975ec2580a to destination ../master bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master

  $ cd ../shallow
  $ hg pull -q
  5 files fetched over 5 fetches - (5 misses, 0.00% hit ratio) over *s (glob) (?)
  $ hg log --removed a.bin --config diff.nobinary=1 --git -p -T '{desc}\n' -r '::tip'
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
  
# comment so we don't need to end with trailing spaces
