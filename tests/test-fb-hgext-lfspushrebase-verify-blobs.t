  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=
  > pushrebase=
  > [lfs]
  > threshold=10B
  > url=file:$TESTTMP/dummy-remote/
  > verify=existance
  > [pushrebase]
  > rewritedates = True
  > [diff]
  > git=1
  > EOF

# prepare a full repo with lfs metadata
  $ hg init master
  $ hg init lfs-upload-trigger
  $ cd master
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
  $ hg book master

  $ hg push -q ../lfs-upload-trigger

  $ cd ..


# clone a client
  $ hg clone ssh://user@dummy/master client --noupdate
  streaming all changes
  4 files to transfer, 1.94 KB of data
  transferred 1.94 KB in * seconds (*) (glob)
  searching for changes
  no changes found
  $ cd client

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -p -r ::tip -T '{rev}:{node} {desc}\n' -G
  @  4:042535657086a5b08463b9210a8f46dc270e51f9 x-lfs-again
  |  diff --git a/x b/x
  |  --- a/x
  |  +++ b/x
  |  @@ -1,1 +1,2 @@
  |   NOTLFS
  |  +BECOME-LFS-AGAIN
  |
  o  3:c6cc0cd58884b847de39aa817ded71e6051caa9f x-nonlfs
  |  diff --git a/y b/x
  |  rename from y
  |  rename to x
  |
  o  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2 y-nonlfs
  |  diff --git a/y b/y
  |  --- a/y
  |  +++ b/y
  |  @@ -1,1 +1,1 @@
  |  -THIS-IS-LFS-FILE
  |  +NOTLFS
  |
  o  1:799bebfa53189a3db8424680f1a8f9806540e541 y-lfs
  |  diff --git a/x b/y
  |  rename from x
  |  rename to y
  |
  o  0:0d2948821b2b3b6e58505696145f2215cea2b2cd x-lfs
     diff --git a/x b/x
     new file mode 100644
     --- /dev/null
     +++ b/x
     @@ -0,0 +1,1 @@
     +THIS-IS-LFS-FILE
  
# Clone a second client
  $ cp -R . ../client2


# Edit the working copy
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


# Push lfs content to server: expect failure, lfs blobs not in server

#if lfs-test-server no-windows
  $ LFS_LISTEN="tcp://:$HGPORT"
  $ LFS_HOST="localhost:$HGPORT"
  $ LFS_PUBLIC=1
  $ export LFS_LISTEN LFS_HOST LFS_PUBLIC
  $ lfs-test-server &> lfs-server.log &
  $ echo $! >> $DAEMON_PIDS

  $ cat >> $TESTTMP/master/.hg/hgrc << EOF
  > [lfs]
  > url=http://test:something@$LFS_HOST/
  > EOF

  $ hg push --to master ../master
  pushing to ../master
  searching for changes
  abort: LFS server error. Remote object for file unknown not found: *u'oid': u'a2fcdb080e9838f6e1476a494c1d553e6ffefb68b0d146a06f34b535b5198442'* (glob)
  [255]

# But push can succeed if the server is configured to skip verifying blobs.
  $ cp -R $TESTTMP/master $TESTTMP/master-no-verify
  $ cp -R $TESTTMP/client $TESTTMP/client-clone
  $ cd $TESTTMP/client-clone
  $ cat >> $TESTTMP/master-no-verify/.hg/hgrc <<EOF
  > [lfs]
  > verify=none
  > EOF

  $ hg push --to master $TESTTMP/master-no-verify
  pushing to $TESTTMP/master-no-verify
  searching for changes
  pushing 1 changeset:
      515a4dfd2e0c  shallow.lfs.commit
  1 new changeset from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files (+1 heads)
  1 new obsolescence markers
  obsoleted 1 changesets
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd $TESTTMP/client

# Reset lfs url
  $ cat >> $TESTTMP/master/.hg/hgrc << EOF
  > [lfs]
  > url=file:$TESTTMP/dummy-remote/
  > EOF


# Push lfs content to server: succeed
  $ hg push --to master ../master
  pushing to ../master
  searching for changes
  pushing 1 changeset:
      515a4dfd2e0c  shallow.lfs.commit
  1 new changeset from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files (+1 heads)
  1 new obsolescence markers
  obsoleted 1 changesets
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved


# Check content
  $ cd ../master
  $ hg log -p -r tip -T '{rev}:{node} {desc}\n'
  5:* shallow.lfs.commit (glob)
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,2 +1,3 @@
   NOTLFS
   BECOME-LFS-AGAIN
  +ADD-A-LINE
  

  $ hg log -T '{rev}:{node} {bookmarks} {desc}\n' -G
  o  5:* shallow.lfs.commit (glob)
  |
  @  4:042535657086a5b08463b9210a8f46dc270e51f9 master x-lfs-again
  |
  o  3:c6cc0cd58884b847de39aa817ded71e6051caa9f  x-nonlfs
  |
  o  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2  y-nonlfs
  |
  o  1:799bebfa53189a3db8424680f1a8f9806540e541  y-lfs
  |
  o  0:0d2948821b2b3b6e58505696145f2215cea2b2cd  x-lfs
  

# Verify the server has lfs content after the pushrebase
  $ hg debugindex y
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     *     -1       1 4ce3392f9979 000000000000 000000000000 (glob)
       1       *       *     -1       2 139f72f7ca98 4ce3392f9979 000000000000 (glob)
       2       *     *     -1       5 f3e0509ec098 000000000000 000000000000 (glob)
  $ hg debugdata y 0
  version https://git-lfs.github.com/spec/v1
  oid sha256:802935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639
  size 17
  x-hg-copy x
  x-hg-copyrev 1ff4e6c9b2764057ea0c52f7b4a5a9be2e79c8e0
  x-is-binary 0
  $ hg debugdata y 1
  NOTLFS
  $ hg debugdata y 2
  version https://git-lfs.github.com/spec/v1
  oid sha256:a2fcdb080e9838f6e1476a494c1d553e6ffefb68b0d146a06f34b535b5198442
  size 35
  x-hg-copy x
  x-hg-copyrev d33b2f7888d4f6f9112256d0f1c625af6d188fde
  x-is-binary 0
  $ hg cat -r 1 y
  THIS-IS-LFS-FILE
  $ hg cat -r 2 y
  NOTLFS
  $ hg cat -r 5 y
  NOTLFS
  BECOME-LFS-AGAIN
  ADD-A-LINE

  $ hg debugindex x
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     *     -1       0 1ff4e6c9b276 000000000000 000000000000 (glob)
       1       *      *     -1       3 68b9378cf5a1 000000000000 000000000000 (glob)
       2       *     *     -1       4 d33b2f7888d4 68b9378cf5a1 000000000000 (glob)
  $ hg debugdata x 0
  version https://git-lfs.github.com/spec/v1
  oid sha256:802935f5411aa569948cd326115b3521107250019b5dbadf0f6ab2aa2d1e4639
  size 17
  x-is-binary 0
  $ hg debugdata x 1
  \x01 (esc)
  copy: y
  copyrev: 139f72f7ca9816bd6b5fdd8b67331458ba11cc0e
  \x01 (esc)
  NOTLFS
  $ hg debugdata x 2
  version https://git-lfs.github.com/spec/v1
  oid sha256:080f1dba758e4406ab1e722e16fc18965ab2b183979432957418173bf983427f
  size 24
  x-is-binary 0


# pull lfs content from server and update
  $ cd ../client2
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets * (glob)

  $ hg update tip
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg log -p -r tip -T '{rev}:{node} {desc}\n'
  5:* shallow.lfs.commit (glob)
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,2 +1,3 @@
   NOTLFS
   BECOME-LFS-AGAIN
  +ADD-A-LINE
  

  $ hg log -T '{rev}:{node} {bookmarks} {desc}\n' -G
  @  5:*  shallow.lfs.commit (glob)
  |
  o  4:042535657086a5b08463b9210a8f46dc270e51f9 master x-lfs-again
  |
  o  3:c6cc0cd58884b847de39aa817ded71e6051caa9f  x-nonlfs
  |
  o  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2  y-nonlfs
  |
  o  1:799bebfa53189a3db8424680f1a8f9806540e541  y-lfs
  |
  o  0:0d2948821b2b3b6e58505696145f2215cea2b2cd  x-lfs
  
#endif
