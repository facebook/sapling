  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=$TESTDIR/../hgext3rd/lfs/
  > [lfs]
  > threshold=10B
  > blobstore=cache/localblobstore
  > remotestore=dummy
  > remotepath=$TESTTMP/dummy-remote/
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

  $ cat >> .hg/hgrc <<EOF
  > [lfs]
  > # "bare" server usually has bypass set. it should also work if bypass=0
  > bypass=1
  > EOF

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

  $ hg log -p -r ::tip -T '{rev}:{node} {desc}\n' --config lfs.bypass=1
  0:0d2948821b2b3b6e58505696145f2215cea2b2cd x-lfs
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,4 @@
  +version https://git-lfs.github.com/spec/v1
  +oid sha256:802935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639
  +size 17
  +x-is-binary 0
  
  1:799bebfa53189a3db8424680f1a8f9806540e541 y-lfs
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,4 +1,6 @@
   version https://git-lfs.github.com/spec/v1
   oid sha256:802935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639
   size 17
  +x-hg-copy x
  +x-hg-copyrev 1ff4e6c9b2764057ea0c52f7b4a5a9be2e79c8e0
   x-is-binary 0
  
  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2 y-nonlfs
  diff --git a/y b/y
  --- a/y
  +++ b/y
  @@ -1,6 +1,1 @@
  -version https://git-lfs.github.com/spec/v1
  -oid sha256:802935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639
  -size 17
  -x-hg-copy x
  -x-hg-copyrev 1ff4e6c9b2764057ea0c52f7b4a5a9be2e79c8e0
  -x-is-binary 0
  +NOTLFS
  
  3:c6cc0cd58884b847de39aa817ded71e6051caa9f x-nonlfs
  diff --git a/y b/x
  rename from y
  rename to x
  
  4:042535657086a5b08463b9210a8f46dc270e51f9 x-lfs-again
  diff --git a/x b/x
  --- a/x
  +++ b/x
  @@ -1,1 +1,4 @@
  -NOTLFS
  +version https://git-lfs.github.com/spec/v1
  +oid sha256:080f1dba758e4406ab1e722e16fc18965ab2b183979432957418173bf983427f
  +size 24
  +x-is-binary 0
  
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
  
  $ hg log -p -r ::tip -T '{rev}:{node} {desc}\n' --config lfs.bypass=1
  0:0d2948821b2b3b6e58505696145f2215cea2b2cd x-lfs
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,4 @@
  +version https://git-lfs.github.com/spec/v1
  +oid sha256:802935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639
  +size 17
  +x-is-binary 0
  
  1:799bebfa53189a3db8424680f1a8f9806540e541 y-lfs
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,4 +1,6 @@
   version https://git-lfs.github.com/spec/v1
   oid sha256:802935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639
   size 17
  +x-hg-copy x
  +x-hg-copyrev 1ff4e6c9b2764057ea0c52f7b4a5a9be2e79c8e0
   x-is-binary 0
  
  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2 y-nonlfs
  diff --git a/y b/y
  --- a/y
  +++ b/y
  @@ -1,6 +1,1 @@
  -version https://git-lfs.github.com/spec/v1
  -oid sha256:802935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639
  -size 17
  -x-hg-copy x
  -x-hg-copyrev 1ff4e6c9b2764057ea0c52f7b4a5a9be2e79c8e0
  -x-is-binary 0
  +NOTLFS
  
  3:c6cc0cd58884b847de39aa817ded71e6051caa9f x-nonlfs
  diff --git a/y b/x
  rename from y
  rename to x
  
  4:042535657086a5b08463b9210a8f46dc270e51f9 x-lfs-again
  diff --git a/x b/x
  --- a/x
  +++ b/x
  @@ -1,1 +1,4 @@
  -NOTLFS
  +version https://git-lfs.github.com/spec/v1
  +oid sha256:080f1dba758e4406ab1e722e16fc18965ab2b183979432957418173bf983427f
  +size 24
  +x-is-binary 0
  
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
  $ hg log -p -r tip -T '{rev}:{node} {desc}\n' --config lfs.bypass=0
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
  
  $ hg log -p -r tip -T '{rev}:{node} {desc}\n' --config lfs.bypass=1
  5:515a4dfd2e0c4c963dcbf4bc48587b9747143598 shallow.lfs.commit
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,4 +1,6 @@
   version https://git-lfs.github.com/spec/v1
  -oid sha256:080f1dba758e4406ab1e722e16fc18965ab2b183979432957418173bf983427f
  -size 24
  +oid sha256:a2fcdb080e9838f6e1476a494c1d553e6ffefb68b0d146a06f34b535b5198442
  +size 35
  +x-hg-copy x
  +x-hg-copyrev d33b2f7888d4f6f9112256d0f1c625af6d188fde
   x-is-binary 0
  
# pull lfs content from server and update

  $ cd ../shallow2
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  (run 'hg update' to get a working copy)

  $ hg update tip
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  1 files fetched over * (glob)

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
  
  $ hg log -p -r tip -T '{rev}:{node} {desc}\n' --config lfs.bypass=1
  5:515a4dfd2e0c4c963dcbf4bc48587b9747143598 shallow.lfs.commit
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,4 +1,6 @@
   version https://git-lfs.github.com/spec/v1
  -oid sha256:080f1dba758e4406ab1e722e16fc18965ab2b183979432957418173bf983427f
  -size 24
  +oid sha256:a2fcdb080e9838f6e1476a494c1d553e6ffefb68b0d146a06f34b535b5198442
  +size 35
  +x-hg-copy x
  +x-hg-copyrev d33b2f7888d4f6f9112256d0f1c625af6d188fde
   x-is-binary 0
  
# repack again

  $ cd ../shallow

  $ hg repack
  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/879f0543e467d3cffb512cc0392ebece41b1480f.dataidx
  $TESTTMP/hgcache/master/packs/879f0543e467d3cffb512cc0392ebece41b1480f.datapack
  $TESTTMP/hgcache/master/packs/bf634767241b49b174b18732f92c6653ff966751.histidx
  $TESTTMP/hgcache/master/packs/bf634767241b49b174b18732f92c6653ff966751.histpack
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
  
