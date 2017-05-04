# Initial setup

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=$TESTDIR/../hgext3rd/lfs/
  > [lfs]
  > threshold=1000B
  > chunksize=1000B
  > blobstore=cache/localblobstore
  > remotestore=dummy
  > remotepath=$TESTTMP/dummy-remote/
  > EOF

  $ LONG=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

# Prepare server and enable extension
  $ hg init server
  $ hg clone -q server client
  $ cd client

# Commit small file
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"

# Commit large file
  $ echo $LONG > largefile
  $ hg commit --traceback -Aqm "add large file"

# Ensure metadata is stored
  $ hg debugdata largefile 0
  version https://git-lfs.github.com/spec/chunking
  chunks d7dbc611df1fe7dfacfe267a2bfd32ba8fc27ad16aa72af7e6c553a120b92f18:1000,ed0f071aa4ff28ab9863b6cfc5f407e915612d70502422e4ab9b09f3dfec4a74:501
  hashalgo sha256
  size 1501
  x-is-binary 0

# Check the blobstore is populated
  $ find .hg/cache/localblobstore | sort
  .hg/cache/localblobstore
  .hg/cache/localblobstore/d7
  .hg/cache/localblobstore/d7/dbc611df1fe7dfacfe267a2bfd32ba8fc27ad16aa72af7e6c553a120b92f18
  .hg/cache/localblobstore/ed
  .hg/cache/localblobstore/ed/0f071aa4ff28ab9863b6cfc5f407e915612d70502422e4ab9b09f3dfec4a74

# Check the blob stored contains the actual contents of the file
  $ cat .hg/cache/localblobstore/d7/dbc611df1fe7dfacfe267a2bfd32ba8fc27ad16aa72af7e6c553a120b92f18
  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB (no-eol)
  $ cat .hg/cache/localblobstore/ed/0f071aa4ff28ab9863b6cfc5f407e915612d70502422e4ab9b09f3dfec4a74
  CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

# Push changes to the server
  $ hg push -v | egrep -v '^(uncompressed| )'
  pushing to $TESTTMP/server (glob)
  searching for changes
  lfs: computing set of blobs to upload
  lfs: need to upload 2 objects (1.47 KB)
  2 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

# Unknown remotestore

  $ hg push --config lfs.remotestore=404
  abort: lfs: unknown remotestore: 404
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
  pulling from $TESTTMP/server (glob)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)

# Check the blobstore is not yet populated
  $ [ -f .hg/cache/localblobstore ]
  [1]

# Update to the last revision containing the large file
  $ hg update
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Check the blobstore has been populated on update
  $ find .hg/cache/localblobstore | sort
  .hg/cache/localblobstore
  .hg/cache/localblobstore/d7
  .hg/cache/localblobstore/d7/dbc611df1fe7dfacfe267a2bfd32ba8fc27ad16aa72af7e6c553a120b92f18
  .hg/cache/localblobstore/ed
  .hg/cache/localblobstore/ed/0f071aa4ff28ab9863b6cfc5f407e915612d70502422e4ab9b09f3dfec4a74

# Check the contents of the file are fetched from blobstore when requested
  $ hg cat -r . largefile
  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

# Check the file has been copied in the working copy
  $ cat largefile
  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

  $ cd ..

# Check blobstore could be an absolute path outside repo vfs:

  $ mkdir -p $TESTTMP/global-store
  $ cat >> $HGRCPATH <<EOF
  > [lfs]
  > blobstore=$TESTTMP/global-store
  > EOF

  $ hg init repo2
  $ cd repo2

  $ echo $LONG > largefile
  $ hg add largefile
  $ hg commit -m initcommit
  $ [ -f $TESTTMP/global-store/ed/0f071aa4ff28ab9863b6cfc5f407e915612d70502422e4ab9b09f3dfec4a74 ]
  $ [ -f $TESTTMP/global-store/d7/dbc611df1fe7dfacfe267a2bfd32ba8fc27ad16aa72af7e6c553a120b92f18 ]

  $ cd ..

# Check rename, and switch between large and small files

  $ hg init repo3
  $ cd repo3
  $ cat >> .hg/hgrc << EOF
  > [lfs]
  > threshold=10B
  > chunksize=10B
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

# Push and pull the above repo

  $ hg --cwd .. init repo4
  $ hg push ../repo4
  pushing to ../repo4
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 10 changes to 4 files

  $ hg --cwd .. init repo5
  $ hg --cwd ../repo5 pull ../repo3
  pulling from ../repo3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 10 changes to 4 files
  (run 'hg update' to get a working copy)

  $ cd ..

# Test clone

  $ hg init repo6
  $ cd repo6
  $ cat >> .hg/hgrc << EOF
  > [lfs]
  > threshold=30B
  > chunksize=20B
  > EOF

  $ echo LARGE-BECAUSE-IT-IS-MORE-THAN-30-BYTES > large
  $ echo SMALL > small
  $ hg commit -Aqm 'create a lfs file' large small

  $ cd ..

  $ hg clone repo6 repo7
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo7
  $ cat large
  LARGE-BECAUSE-IT-IS-MORE-THAN-30-BYTES
  $ cat small
  SMALL

# Test bypass

  $ hg cat -r tip large --config lfs.remotepath=$TESTTMP/404 --config lfs.blobstore=cache/404
  abort: No such file or directory: $TESTTMP/404/2e81056070ae365867a5e7f804abe39c12e07bdf11c098994d1bc0ab9981910a
  [255]

  $ hg cat -r tip large --config lfs.remotepath=$TESTTMP/404 --config lfs.blobstore=cache/404 --config lfs.bypass=1
  version https://git-lfs.github.com/spec/chunking
  chunks 2e81056070ae365867a5e7f804abe39c12e07bdf11c098994d1bc0ab9981910a:20,903f301311488de7befd35be124433f73d3b7b6bce1d75440474907ff2ae7591:19
  hashalgo sha256
  size 39
  x-is-binary 0

  $ cd ..

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
  $ HEADER=$'\1\n'
  $ printf '%sSTART-WITH-HG-FILELOG-METADATA' "$HEADER" > a2
  $ printf '%sMETA\n' "$HEADER" > a1
  $ hg commit -m meta
  $ hg status
  $ hg log -T '{rev}: {file_copies} | {file_dels} | {file_adds}\n'
  2:  |  | 
  1: a1 (a2)a2 (a1) |  | 
  0:  |  | a1 a2

  $ for n in a1 a2; do
  >   for r in 0 1 2; do
  >     printf '\n%s @ %s\n' $n $r
  >     hg debugdata $n $r
  >   done
  > done
  
  a1 @ 0
  version https://git-lfs.github.com/spec/v1
  oid sha256:5bb8341bee63b3649f222b2215bde37322bea075a30575aa685d8f8d21c77024
  size 29
  x-is-binary 0
  
  a1 @ 1
  \x01 (esc)
  copy: a2
  copyrev: 50470ad23cf937b1f4b9f80bfe54df38e65b50d9
  \x01 (esc)
  SMALL
  
  a1 @ 2
  \x01 (esc)
  \x01 (esc)
  \x01 (esc)
  META
  
  a2 @ 0
  SMALL
  
  a2 @ 1
  version https://git-lfs.github.com/spec/v1
  oid sha256:5bb8341bee63b3649f222b2215bde37322bea075a30575aa685d8f8d21c77024
  size 29
  x-hg-copy a1
  x-hg-copyrev be23af27908a582af43e5cda209a5a9b319de8d4
  x-is-binary 0
  
  a2 @ 2
  version https://git-lfs.github.com/spec/v1
  oid sha256:876dadc86a8542f9798048f2c47f51dbf8e4359aed883e8ec80c5db825f0d943
  size 32
  x-is-binary 0

# Verify commit hashes include rename metadata

  $ hg log -T '{rev}:{node|short} {desc}\n'
  2:0fae949de7fa meta
  1:9cd6bdffdac0 b
  0:7f96794915f7 a

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

  $ hg update 2 -q
  $ echo 'THIS-IS-LFS-2-CHILD' > a
  $ hg commit -m branching -q

  $ hg bundle --base 1 bundle.hg
  4 changesets found
  $ hg --config extensions.strip= strip -r 2 --no-backup --force -q
  $ hg -R bundle.hg log -p -T '{rev} {desc}\n' a
  5 branching
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-two
  +THIS-IS-LFS-2-CHILD
  
  4 a-4
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-three
  +THIS-IS-LFS-4
  
  3 a-three
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-two
  +THIS-IS-LFS-three
  
  2 a-two
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-single
  +THIS-IS-LFS-two
  
  1 a-single
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-0
  +THIS-IS-LFS-single
  
  0 a-0
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +THIS-IS-LFS-0
  
  $ hg bundle -R bundle.hg --base 1 bundle-again.hg -q
  $ hg -R bundle-again.hg log -p -T '{rev} {desc}\n' a
  5 branching
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-two
  +THIS-IS-LFS-2-CHILD
  
  4 a-4
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-three
  +THIS-IS-LFS-4
  
  3 a-three
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-two
  +THIS-IS-LFS-three
  
  2 a-two
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-single
  +THIS-IS-LFS-two
  
  1 a-single
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -THIS-IS-LFS-0
  +THIS-IS-LFS-single
  
  0 a-0
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
  $ hg commit -m binarytest -A a b c d
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
  b55353847f02 tip

  $ cd ..

# Verify the repos

  $ cat > $TESTTMP/dumpflog.py << EOF
  > # print raw revision sizes, flags, and hashes for certain files
  > import hashlib
  > from mercurial import revlog
  > from mercurial.node import short
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
  >         flags = [fl.flags(i) for i in fl]
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
    l: rawsizes=[435, 6, 8, 230] flags=[8192, 0, 0, 8192] hashes=['45db', '948c', 'cc88', '0051']
    s: rawsizes=[74, 297, 297, 8] flags=[0, 8192, 8192, 0] hashes=['3c80', '9a56', '2573', '826b']
  repo: repo4
    l: rawsizes=[435, 6, 8, 230] flags=[8192, 0, 0, 8192] hashes=['45db', '948c', 'cc88', '0051']
    s: rawsizes=[74, 297, 297, 8] flags=[0, 8192, 8192, 0] hashes=['3c80', '9a56', '2573', '826b']
  repo: repo5
    l: rawsizes=[435, 6, 8, 230] flags=[8192, 0, 0, 8192] hashes=['45db', '948c', 'cc88', '0051']
    s: rawsizes=[74, 297, 297, 8] flags=[0, 8192, 8192, 0] hashes=['3c80', '9a56', '2573', '826b']
  repo: repo6
  repo: repo7
  repo: repo8
  repo: repo9
  repo: repo10
