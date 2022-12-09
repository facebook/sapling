  $ setconfig workingcopy.ruststatus=False
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ disable treemanifest
  $ setconfig experimental.allowfilepeer=True
# Initial setup

  $ enable lfs
  $ setconfig lfs.threshold=1000B lfs.usercache="$TESTTMP/lfs-cache"

  $ LONG=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

# Prepare server and client repos. lfs.usercache can be empty
  $ hg init server
  $ cat >> server/.hg/hgrc << EOF
  > [lfs]
  > usercache=
  > EOF
  $ hg clone -q server client
  $ cd client

# Commit small file
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"

# Commit large file
  $ echo $LONG > largefile
  $ grep lfs .hg/requires
  [1]
  $ hg commit --traceback -Aqm "add large file"
  $ grep lfs .hg/requires
  lfs

# Ensure metadata is stored
  $ hg debugfilerev largefile -v
  00c137947d30: add large file
   largefile: bin=0 lnk=0 flag=2000 size=1501 copied='' chain=1c509d1a5c8a
    rawdata: 'version https://git-lfs.github.com/spec/v1\noid sha256:f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b\nsize 1501\nx-is-binary 0\n'

# Check the blobstore is populated
  $ find .hg/store/lfs/objects | sort
  .hg/store/lfs/objects
  .hg/store/lfs/objects/f1
  .hg/store/lfs/objects/f1/1e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b

# Check the blob stored contains the actual contents of the file
  $ cat .hg/store/lfs/objects/f1/1e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b
  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

# Push changes to the server

  $ hg push
  pushing to $TESTTMP/server
  searching for changes
  abort: lfs.url needs to be configured
  [255]

  $ cat >> $HGRCPATH << EOF
  > [lfs]
  > url=file:$TESTTMP/dummy-remote/
  > EOF

  $ hg push -v 2>&1 | egrep -v '^(uncompressed| )'
  pushing to $TESTTMP/server
  searching for changes
  2 changesets found
  adding changesets
  adding manifests
  adding file changes

# Unknown URL scheme

  $ hg push --config lfs.url=ftp://foobar
  abort: lfs: unknown url scheme: ftp
  [255]

  $ cd ../

# Initialize new client (not cloning) and setup extension
  $ hg init client2
  $ cd client2
  $ cat >> .hg/hgrc <<EOF
  > [paths]
  > default = $TESTTMP/server
  > EOF

# Pull from server
  $ hg pull default
  pulling from $TESTTMP/server
  requesting all changes
  adding changesets
  adding manifests
  adding file changes

# Check the blobstore is not yet populated
  $ [ -d .hg/store/lfs/objects ]
  [1]

# Update to the last revision containing the large file
  $ hg goto
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Check the blobstore has been populated on update
  $ find .hg/store/lfs/objects | sort
  .hg/store/lfs/objects
  .hg/store/lfs/objects/f1
  .hg/store/lfs/objects/f1/1e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b

# Check the contents of the file are fetched from blobstore when requested
  $ hg cat -r . largefile
  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

# Check the file has been copied in the working copy
  $ cat largefile
  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

  $ cd ..

# Check rename, and switch between large and small files

  $ hg init repo3
  $ cd repo3
  $ cat >> .hg/hgrc << EOF
  > [lfs]
  > threshold=10B
  > EOF

  $ echo LONGER-THAN-TEN-BYTES-WILL-TRIGGER-LFS > large
  $ echo SHORTER > small
  $ hg add . -q
  $ hg commit -m 'commit with lfs content'

  $ hg mv large l
  $ hg mv small s
  $ hg commit -m 'renames'

  $ echo SHORT > l
  $ echo BECOME-LARGER-FROM-SHORTER > s
  $ hg commit -m 'large to small, small to large'

  $ echo 1 >> l
  $ echo 2 >> s
  $ hg commit -m 'random modifications'

  $ echo RESTORE-TO-BE-LARGE > l
  $ echo SHORTER > s
  $ hg commit -m 'switch large and small again'

  $ hg debugfilerev -r 'all()'
  fd47a419c4f7: commit with lfs content
   large: bin=0 lnk=0 flag=2000 size=39 copied='' chain=2c531e0992ff
   small: bin=0 lnk=0 flag=0 size=8 copied='' chain=b92a1ddc2cb0
  514ca5454649: renames
   l: bin=0 lnk=0 flag=2000 size=39 copied='large' chain=46a2f24864bc
   s: bin=0 lnk=0 flag=0 size=8 copied='small' chain=594f4fdf95ce
  e8e237bfd98f: large to small, small to large
   l: bin=0 lnk=0 flag=0 size=6 copied='' chain=b484bd96359a
   s: bin=0 lnk=0 flag=2000 size=27 copied='' chain=2521c65ce463
  15c00ca48977: random modifications
   l: bin=0 lnk=0 flag=0 size=8 copied='' chain=8f150b4b7e9f
   s: bin=0 lnk=0 flag=2000 size=29 copied='' chain=552783341059
  5adf850972b9: switch large and small again
   l: bin=0 lnk=0 flag=2000 size=20 copied='' chain=6f1ff1f39c11
   s: bin=0 lnk=0 flag=0 size=8 copied='' chain=0c1fa52a67c6

# Test lfs_files template

  $ hg log -r 'all()' -T '{join(lfs_files, ", ")}\n'
  large
  l
  s
  s
  l

# Push and pull the above repo

  $ hg --cwd .. init repo4
  $ hg push ../repo4
  pushing to ../repo4
  searching for changes
  adding changesets
  adding manifests
  adding file changes

  $ hg --cwd .. init repo5
  $ hg --cwd ../repo5 pull ../repo3
  pulling from ../repo3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes

  $ cd ..

# Test clone

  $ hg init repo6
  $ cd repo6
  $ cat >> .hg/hgrc << EOF
  > [lfs]
  > threshold=30B
  > EOF

  $ echo LARGE-BECAUSE-IT-IS-MORE-THAN-30-BYTES > large
  $ echo SMALL > small
  $ hg commit -Aqm 'create a lfs file' large small
  $ hg debuglfsupload -r 'all()' -v

  $ cd ..

  $ hg clone repo6 repo7
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo7
  $ hg config extensions --debug | grep ':.*lfs'
  $TESTTMP/repo7/.hg/hgrc:*: extensions.lfs= (glob)
  $ cat large
  LARGE-BECAUSE-IT-IS-MORE-THAN-30-BYTES
  $ cat small
  SMALL

  $ cd ..

  $ hg --config extensions.share= share repo7 sharedrepo
  updating working directory
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R sharedrepo config extensions --debug | grep ':.*lfs'
  $TESTTMP/sharedrepo/.hg/hgrc:*: extensions.lfs= (glob)

# Test rename and status

  $ hg init repo8
  $ cd repo8
  $ cat >> .hg/hgrc << EOF
  > [lfs]
  > threshold=10B
  > EOF

  $ echo THIS-IS-LFS-BECAUSE-10-BYTES > a1
  $ echo SMALL > a2
  $ hg commit -m a -A a1 a2
  $ hg status
  $ hg mv a1 b1
  $ hg mv a2 a1
  $ hg mv b1 a2
  $ hg commit -m b
  $ hg status
  >>> with open('a2', 'wb') as f:
  ...     _ = f.write(b'\1\nSTART-WITH-HG-FILELOG-METADATA')
  >>> with open('a1', 'wb') as f:
  ...     _ = f.write(b'\1\nMETA\n')
  $ hg commit -m meta
  $ hg status
  $ hg log -T '{file_copies} | {file_dels} | {file_adds}\n'
   |  | 
  a1 (a2)a2 (a1) |  | 
   |  | a1 a2

# Verify commit hashes include rename metadata

  $ hg log -T '{node|short} {desc}\n'
  0fae949de7fa meta
  9cd6bdffdac0 b
  7f96794915f7 a

  $ hg debugfilerev -r 'all()' -v
  7f96794915f7: a
   a1: bin=0 lnk=0 flag=2000 size=29 copied='' chain=be23af27908a
    rawdata: 'version https://git-lfs.github.com/spec/v1\noid sha256:5bb8341bee63b3649f222b2215bde37322bea075a30575aa685d8f8d21c77024\nsize 29\nx-is-binary 0\n'
   a2: bin=0 lnk=0 flag=0 size=6 copied='' chain=50470ad23cf9
    rawdata: 'SMALL\n'
  9cd6bdffdac0: b
   a1: bin=0 lnk=0 flag=0 size=6 copied='a2' chain=0d759f317f5a
    rawdata: '\x01\ncopy: a2\ncopyrev: 50470ad23cf937b1f4b9f80bfe54df38e65b50d9\n\x01\nSMALL\n'
   a2: bin=0 lnk=0 flag=2000 size=29 copied='a1' chain=b982e9429db8
    rawdata: 'version https://git-lfs.github.com/spec/v1\noid sha256:5bb8341bee63b3649f222b2215bde37322bea075a30575aa685d8f8d21c77024\nsize 29\nx-hg-copy a1\nx-hg-copyrev be23af27908a582af43e5cda209a5a9b319de8d4\nx-is-binary 0\n'
  0fae949de7fa: meta
   a1: bin=0 lnk=0 flag=0 size=11 copied='' chain=0984adb90885
    rawdata: '\x01\n\x01\n\x01\nMETA\n'
   a2: bin=0 lnk=0 flag=2000 size=32 copied='' chain=7691bcc594f0
    rawdata: 'version https://git-lfs.github.com/spec/v1\noid sha256:876dadc86a8542f9798048f2c47f51dbf8e4359aed883e8ec80c5db825f0d943\nsize 32\nx-is-binary 0\n'

  $ cd ..

# Test bundle

  $ hg init repo9
  $ cd repo9
  $ cat >> .hg/hgrc << EOF
  > [lfs]
  > threshold=10B
  > [diff]
  > git=1
  > EOF

  $ for i in 0 single two three 4; do
  >   echo 'THIS-IS-LFS-'$i > a
  >   hg commit -m a-$i -A a
  > done

  $ hg goto 5b495c34b2630950b01ace9083c5260430bd2d52 -q
  $ echo 'THIS-IS-LFS-2-CHILD' > a
  $ hg commit -m branching -q

  $ hg bundle --base 045a1dfffcffc2f7f47eadef15b658a8402082b9 bundle.hg -v
  4 changesets found
  uncompressed size of bundle content:
       * (changelog) (glob)
       * (manifests) (glob)
      * a (glob)
  $ hg debugstrip -r 5b495c34b2630950b01ace9083c5260430bd2d52 --no-backup --force -q
  $ hg -R bundle.hg debugfilerev -r 'bundle()'
  5b495c34b263: a-two
   a: bin=0 lnk=0 flag=2000 size=16 copied='' chain=4b09ab2030a1
  a887db3fdadc: a-three
   a: bin=0 lnk=0 flag=2000 size=18 copied='' chain=d0f2c6cc8434
  617b75df8389: a-4
   a: bin=0 lnk=0 flag=2000 size=14 copied='' chain=47910e2096f9
  8317d37315be: branching
   a: bin=0 lnk=0 flag=2000 size=20 copied='' chain=d55fa5808479
  $ hg -R bundle.hg log -p -T '{desc}\n' a
  branching
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-two
  +THIS-IS-LFS-2-CHILD
  
  a-4
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-three
  +THIS-IS-LFS-4
  
  a-three
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-two
  +THIS-IS-LFS-three
  
  a-two
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-single
  +THIS-IS-LFS-two
  
  a-single
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-0
  +THIS-IS-LFS-single
  
  a-0
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-0
  
  $ hg bundle -R bundle.hg --base 045a1dfffcffc2f7f47eadef15b658a8402082b9 bundle-again.hg -q
  $ hg -R bundle-again.hg log -p -T '{desc}\n' a
  branching
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-two
  +THIS-IS-LFS-2-CHILD
  
  a-4
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-three
  +THIS-IS-LFS-4
  
  a-three
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-two
  +THIS-IS-LFS-three
  
  a-two
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-single
  +THIS-IS-LFS-two
  
  a-single
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-0
  +THIS-IS-LFS-single
  
  a-0
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-0
  
  $ cd ..

# Test isbinary

  $ hg init repo10
  $ cd repo10
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=1
  > EOF
  $ $PYTHON <<'EOF'
  > def write(path, content):
  >     with open(path, 'wb') as f:
  >         f.write(content)
  > write('a', b'\0\0')
  > write('b', b'\1\n')
  > write('c', b'\1\n\0')
  > write('d', b'xx')
  > EOF
  $ hg add a b c d
  $ hg diff --stat
   a |  Bin 
   b |    1 +
   c |  Bin 
   d |    1 +
   4 files changed, 2 insertions(+), 0 deletions(-)
  $ hg commit -m binarytest
  $ cat > $TESTTMP/dumpbinary.py << EOF
  > def reposetup(ui, repo):
  >     for n in 'abcd':
  >         ui.write(('%s: binary=%s\n') % (n, repo['.'][n].isbinary()))
  > EOF
  $ hg --config extensions.dumpbinary=$TESTTMP/dumpbinary.py id --trace
  a: binary=True
  b: binary=False
  c: binary=True
  d: binary=False
  b55353847f02

  $ cd ..

# Test fctx.cmp fastpath - diff without LFS blobs

  $ hg init repo11
  $ cd repo11
  $ cat >> .hg/hgrc <<EOF
  > [lfs]
  > threshold=1
  > EOF
  $ cat > ../patch.diff <<EOF
  > # HG changeset patch
  > 2
  > 
  > diff --git a/a b/a
  > old mode 100644
  > new mode 100755
  > EOF

  $ for i in 1 2 3; do
  >     cp ../repo10/a a
  >     if [ $i = 3 ]; then
  >         # make a content-only change
  >         hg import -q --bypass ../patch.diff
  >         hg goto -q
  >         rm ../patch.diff
  >     else
  >         echo $i >> a
  >         hg commit -m $i -A a
  >     fi
  > done
  $ [ -d .hg/store/lfs/objects ]

  $ cd ..

  $ hg clone repo11 repo12 --noupdate
  $ cd repo12
  $ hg log --removed -p a -T '{desc}\n' --config diff.nobinary=1 --git
  2
  diff --git a/a b/a
  old mode 100644
  new mode 100755
  
  2
  diff --git a/a b/a
  Binary file a has changed
  
  1
  diff --git a/a b/a
  new file mode 100644
  Binary file a has changed
  
  $ [ -d .hg/store/lfs/objects ]
  [1]

  $ cd ..

# Verify the repos

  $ cat > $TESTTMP/dumpflog.py << EOF
  > # print raw revision sizes, flags, and hashes for certain files
  > import hashlib
  > from edenscm import revlog
  > from edenscm.node import short
  > def hash(rawtext):
  >     h = hashlib.sha512()
  >     h.update(rawtext)
  >     return h.hexdigest()[:4]
  > def reposetup(ui, repo):
  >     # these 2 files are interesting
  >     for name in ['l', 's']:
  >         fl = repo.file(name)
  >         if len(fl) == 0:
  >             continue
  >         sizes = [revlog.revlog.rawsize(fl, i) for i in fl]
  >         texts = [fl.revision(i, raw=True) for i in fl]
  >         flags = [int(fl.flags(i)) for i in fl]
  >         hashes = [hash(t) for t in texts]
  >         print('  %s: rawsizes=%r flags=%r hashes=%r'
  >               % (name, sizes, flags, hashes))
  > EOF

  $ for i in client client2 server repo3 repo4 repo5 repo6 repo7 repo8 repo9 \
  >          repo10; do
  >   echo 'repo:' $i
  >   hg --cwd $i verify --config extensions.dumpflog=$TESTTMP/dumpflog.py -q
  > done
  repo: client
  repo: client2
  repo: server
  repo: repo3
    l: rawsizes=[211, 6, 8, 141] flags=[8192, 0, 0, 8192] hashes=['d2b8', '948c', 'cc88', '724d']
    s: rawsizes=[74, 141, 141, 8] flags=[0, 8192, 8192, 0] hashes=['3c80', 'fce0', '874a', '826b']
  repo: repo4
    l: rawsizes=[211, 6, 8, 141] flags=[8192, 0, 0, 8192] hashes=['d2b8', '948c', 'cc88', '724d']
    s: rawsizes=[74, 141, 141, 8] flags=[0, 8192, 8192, 0] hashes=['3c80', 'fce0', '874a', '826b']
  repo: repo5
    l: rawsizes=[211, 6, 8, 141] flags=[8192, 0, 0, 8192] hashes=['d2b8', '948c', 'cc88', '724d']
    s: rawsizes=[74, 141, 141, 8] flags=[0, 8192, 8192, 0] hashes=['3c80', 'fce0', '874a', '826b']
  repo: repo6
  repo: repo7
  repo: repo8
  repo: repo9
  repo: repo10

repo12 doesn't have any cached lfs files and its source never pushed its
files.  Therefore, the files don't exist in the remote store.  Use the files in
the user cache.

  $ test -d $TESTTMP/repo12/.hg/store/lfs/objects
  [1]

  $ hg --config extensions.share= share repo12 repo13
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R repo13 -q verify

  $ hg clone repo12 repo14
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R repo14 -q verify

If the source repo doesn't have the blob (maybe it was pulled or cloned with
--noupdate), the blob is still accessible via the global cache to send to the
remote store.

  $ rm -rf $TESTTMP/repo14/.hg/store/lfs
  $ hg init repo15
  $ hg -R repo14 push repo15
  pushing to repo15
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg -R repo14 -q verify

Committing deleted files works:

  $ hg init $TESTTMP/repo-del
  $ cd $TESTTMP/repo-del
  $ echo 1 > A
  $ hg commit -m 'add A' -A A
  $ hg rm A
  $ hg commit -m 'rm A'
