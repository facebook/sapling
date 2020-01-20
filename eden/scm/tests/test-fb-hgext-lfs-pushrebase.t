#chg-compatible

  $ disable treemanifest
TODO: configure mutation
  $ configure noevolution

  $ . "$TESTDIR/library.sh"

  $ enable lfs pushrebase
  $ setconfig diff.git=1 pushrebase.rewritedates=true
  $ readconfig <<EOF
  > [lfs]
  > threshold=10B
  > url=file:$TESTTMP/dummy-remote/
  > verify=existance
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
  5 files to transfer, * of data (glob)
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

# Introduce conflict in x in master

  $ cd ../master
  $ hg log -r master -T '{rev}:{node}\n'
  4:042535657086a5b08463b9210a8f46dc270e51f9

  $ echo INTRODUCE-CONFLICT >> x
  $ hg commit -qm introduce-conflict
  $ hg log -r master -T '{rev}:{node}\n'
  5:949778ec92dd45fafbcc6ab7b8c843fdceb66e24
  $ hg book conflict_master -r 949778ec92dd45fafbcc6ab7b8c843fdceb66e24

  $ hg update -q 042535657086a5b08463b9210a8f46dc270e51f9
  $ echo NEW-FILE >> z
  $ hg commit -qAm add-new-file
  $ hg log -r tip -T '{rev}:{node}\n'
  6:a5bcdd7fe9f0d300a2dbed2bd81d140547638856
  $ hg book -f -r a5bcdd7fe9f0d300a2dbed2bd81d140547638856 master

  $ hg log -T '{rev}:{node} {bookmarks} {desc}\n' -G
  @  6:a5bcdd7fe9f0d300a2dbed2bd81d140547638856 master add-new-file
  |
  | o  5:949778ec92dd45fafbcc6ab7b8c843fdceb66e24 conflict_master introduce-conflict
  |/
  o  4:042535657086a5b08463b9210a8f46dc270e51f9  x-lfs-again
  |
  o  3:c6cc0cd58884b847de39aa817ded71e6051caa9f  x-nonlfs
  |
  o  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2  y-nonlfs
  |
  o  1:799bebfa53189a3db8424680f1a8f9806540e541  y-lfs
  |
  o  0:0d2948821b2b3b6e58505696145f2215cea2b2cd  x-lfs
  

# Push lfs content to server: conflict in x expected

  $ cd ../client
  $ hg push --to conflict_master ../master
  pushing to ../master
  searching for changes
  abort: conflicting changes in:
      x
  (pull and rebase your changes locally, then try again)
  [255]

# Push lfs content to server: succeed

  $ mv $TESTTMP/master/.hg/store/lfs{,.bak}

  $ hg push --to master ../master
  pushing to ../master
  searching for changes
  pushing 1 changeset:
      515a4dfd2e0c  shallow.lfs.commit
  2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files


# This should work even if the HG server does not have access to LFS server

  $ test -d $TESTTMP/master/.hg/store/lfs
  [1]

# Check content

  $ cd ../master
  $ hg log -T '{rev}:{node} {bookmarks} {desc}\n' -G
  o  7:* shallow.lfs.commit (glob)
  |
  @  6:a5bcdd7fe9f0d300a2dbed2bd81d140547638856 master add-new-file
  |
  | o  5:949778ec92dd45fafbcc6ab7b8c843fdceb66e24 conflict_master introduce-conflict
  |/
  o  4:042535657086a5b08463b9210a8f46dc270e51f9  x-lfs-again
  |
  o  3:c6cc0cd58884b847de39aa817ded71e6051caa9f  x-nonlfs
  |
  o  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2  y-nonlfs
  |
  o  1:799bebfa53189a3db8424680f1a8f9806540e541  y-lfs
  |
  o  0:0d2948821b2b3b6e58505696145f2215cea2b2cd  x-lfs
  
  $ hg log -p -r tip -T '{rev}:{node} {desc}\n'
  7:* shallow.lfs.commit (glob)
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,2 +1,3 @@
   NOTLFS
   BECOME-LFS-AGAIN
  +ADD-A-LINE
  
  $ hg book -f -r tip master

# Verify the server has lfs content after the pushrebase

  $ hg debugindex y
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     *     -1       1 4ce3392f9979 000000000000 000000000000 (glob)
       1       *       *     -1       2 139f72f7ca98 4ce3392f9979 000000000000 (glob)
       2       *     *     -1       7 f3e0509ec098 000000000000 000000000000 (glob)
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
  $ hg cat -r 7 y
  NOTLFS
  BECOME-LFS-AGAIN
  ADD-A-LINE

  $ hg debugindex x
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     *     -1       0 1ff4e6c9b276 000000000000 000000000000 (glob)
       1       *      *     -1       3 68b9378cf5a1 000000000000 000000000000 (glob)
       2       *     *     -1       4 d33b2f7888d4 68b9378cf5a1 000000000000 (glob)
       3       *     *     -1       5 d587d4396479 d33b2f7888d4 000000000000 (glob)

# pull lfs content from server and update

  $ cd ../client2
  $ hg pull --bookmark master
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  updating bookmark master

  $ hg update tip
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -T '{rev}:{node} {bookmarks} {desc}\n' -G
  @  6:* master shallow.lfs.commit (glob)
  |
  o  5:a5bcdd7fe9f0d300a2dbed2bd81d140547638856  add-new-file
  |
  o  4:042535657086a5b08463b9210a8f46dc270e51f9  x-lfs-again
  |
  o  3:c6cc0cd58884b847de39aa817ded71e6051caa9f  x-nonlfs
  |
  o  2:f3dec7f3610207dbf222ec2d7b68df16a5fde0f2  y-nonlfs
  |
  o  1:799bebfa53189a3db8424680f1a8f9806540e541  y-lfs
  |
  o  0:0d2948821b2b3b6e58505696145f2215cea2b2cd  x-lfs
  

  $ hg log -p -r tip -T '{rev}:{node} {desc}\n'
  6:* shallow.lfs.commit (glob)
  diff --git a/x b/y
  rename from x
  rename to y
  --- a/x
  +++ b/y
  @@ -1,2 +1,3 @@
   NOTLFS
   BECOME-LFS-AGAIN
  +ADD-A-LINE
  
  $ cd ..

# Test pushing a file that shrunk from lfs to non-lfs

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > remotenames=
  > [lfs]
  > url=
  > verify=none
  > [pushrebase]
  > rewritedates = False
  > EOF

  $ hg init master2
  $ hg clone -q ssh://user@dummy/master2 client3
  $ cd client3
  $ cat >> .hg/hgrc <<EOF
  > [lfs]
  > url=file:$TESTTMP/dummy-remote/
  > EOF
  $ echo THIS-IS-LFS-FILE > x
  $ hg commit -qAm x-lfs
  $ hg push -q --create --to master -r .
  $ hg pull -f -q
  $ hg up master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg mv x y
  $ hg commit -m y-lfs
  $ hg push -q --to master -r .
  $ hg up master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Cannot push a change that goes from lfs to not-lfs where the previous file
# revision is a rename. This is because internally the server needs to compare
# contents since the rename prevents it from comparing hashes.

  $ echo NOT-LFS > y
  $ hg commit -m y-not-lfs
  $ hg push --to master -r .
  pushing rev 026f7366f3d2 to destination ssh://user@dummy/master2 bookmark master
  searching for changes
  remote: pushing 1 changeset:
  remote:     026f7366f3d2  y-not-lfs
  remote: lfs.url needs to be configured
  abort: push failed on remote
  [255]

# Can push once server has lfs.url set

  $ cat >> ../master2/.hg/hgrc <<EOF
  > [lfs]
  > url=file:$TESTTMP/dummy-remote/
  > EOF
  $ hg push --to master -r .
  pushing rev 026f7366f3d2 to destination ssh://user@dummy/master2 bookmark master
  searching for changes
  remote: pushing 1 changeset:
  remote:     026f7366f3d2  y-not-lfs
  updating bookmark master
