XXX: This test is currently broken if lfs-test-server is installed.

#require false lfs-test-server

  $ setconfig lfs.usercache=$TESTTMP/lfs-cache
  $ LFS_LISTEN="tcp://:$HGPORT"
  $ LFS_HOST="localhost:$HGPORT"
  $ LFS_PUBLIC=1
  $ export LFS_LISTEN LFS_HOST LFS_PUBLIC
#if no-windows
  $ lfs-test-server &> lfs-server.log &
  $ echo $! >> $DAEMON_PIDS
  $ disown %+
#else
  $ cat >> $TESTTMP/spawn.py <<EOF
  > import os
  > import subprocess
  > import sys
  >
  > for path in os.environ["PATH"].split(os.pathsep):
  >     exe = os.path.join(path, 'lfs-test-server.exe')
  >     if os.path.exists(exe):
  >         with open('lfs-server.log', 'wb') as out:
  >             p = subprocess.Popen(exe, stdout=out, stderr=out)
  >             sys.stdout.write('%s\n' % p.pid)
  >             sys.exit(0)
  > sys.exit(1)
  > EOF
  $ $PYTHON $TESTTMP/spawn.py >> $DAEMON_PIDS
#endif

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > lfs=
  > [lfs]
  > url=http://foo:bar@$LFS_HOST/
  > threshold=1
  > verify=existance
  > EOF

  $ hg init repo1
  $ cd repo1
  $ echo THIS-IS-LFS > a
  $ hg commit -m a -A a

  $ hg init ../repo2
  $ hg push ../repo2 -v
  pushing to ../repo2
  searching for changes
  lfs: uploading 31cf46fbc4ecd458a0943c5b4881f1f5a6dd36c53d6167d5b69ac45149b38e5b (12 bytes)
  lfs: processed: 31cf46fbc4ecd458a0943c5b4881f1f5a6dd36c53d6167d5b69ac45149b38e5b
  1 changesets found
  uncompressed size of bundle content:
       * (changelog) (glob)
       * (manifests) (glob)
       *  a (glob)
  adding changesets
  adding manifests
  adding file changes

Clear the cache to force a download
  $ rm -rf `hg config lfs.usercache`
  $ cd ../repo2
  $ hg goto tip -v
  resolving manifests
  getting a
  lfs: downloading 31cf46fbc4ecd458a0943c5b4881f1f5a6dd36c53d6167d5b69ac45149b38e5b (12 bytes)
  lfs: processed: 31cf46fbc4ecd458a0943c5b4881f1f5a6dd36c53d6167d5b69ac45149b38e5b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

When the server has some blobs already

  $ hg mv a b
  $ echo ANOTHER-LARGE-FILE > c
  $ echo ANOTHER-LARGE-FILE2 > d
  $ hg commit -m b-and-c -A b c d
  $ hg push ../repo1 -v | grep -v '^  '
  pushing to ../repo1
  searching for changes
  lfs: need to transfer 2 objects (39 bytes)
  lfs: uploading d11e1a642b60813aee592094109b406089b8dff4cb157157f753418ec7857998 (19 bytes)
  lfs: processed: d11e1a642b60813aee592094109b406089b8dff4cb157157f753418ec7857998
  lfs: uploading 37a65ab78d5ecda767e8622c248b5dbff1e68b1678ab0e730d5eb8601ec8ad19 (20 bytes)
  lfs: processed: 37a65ab78d5ecda767e8622c248b5dbff1e68b1678ab0e730d5eb8601ec8ad19
  1 changesets found
  uncompressed size of bundle content:
  adding changesets
  adding manifests
  adding file changes

Fail to push if LFS blob is not uploaded to the server
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "lfs=" >> .hg/hgrc
  $ echo "[lfs]" >> .hg/hgrc
  $ echo "url=file://$TESTTMP/unused-dummystore" >> .hg/hgrc

  $ echo ANOTHER-LFS > f
  $ hg commit -m f -A f
  $ hg push ../repo1
  pushing to ../repo1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  transaction abort!
  rollback completed
  abort: LFS server error. Remote object for file unknown not found: *u'oid': u'384d99297e974dab7d66361be0276032f5045185d6ce42601f43973e721f1dd9'* (glob)
  [255]

  $ rm .hg/hgrc

Clear the cache to force a download
  $ rm -rf `hg config lfs.usercache`
  $ hg --repo ../repo1 update tip -v
  resolving manifests
  getting b
  getting c
  lfs: downloading d11e1a642b60813aee592094109b406089b8dff4cb157157f753418ec7857998 (19 bytes)
  lfs: processed: d11e1a642b60813aee592094109b406089b8dff4cb157157f753418ec7857998
  getting d
  lfs: downloading 37a65ab78d5ecda767e8622c248b5dbff1e68b1678ab0e730d5eb8601ec8ad19 (20 bytes)
  lfs: processed: 37a65ab78d5ecda767e8622c248b5dbff1e68b1678ab0e730d5eb8601ec8ad19
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Check error message when the remote missed a blob:

  $ echo FFFFF > b
  $ hg commit -m b -A b
  $ echo FFFFF >> b
  $ hg commit -m b b
  $ rm -rf .hg/store/lfs
  $ rm -rf `hg config lfs.usercache`
  $ hg goto -C '.^'
  abort: LFS server error. Remote object for file data/b.i not found: * (glob)
  [255]

Check error message when object does not exist:

  $ cd $TESTTMP
  $ hg init test && cd test
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "lfs=" >> .hg/hgrc
  $ echo "[lfs]" >> .hg/hgrc
  $ echo "threshold=1" >> .hg/hgrc
  $ echo a > a
  $ hg add a
  $ hg commit -m 'test'
  $ echo aaaaa > a
  $ hg commit -m 'largefile'
  $ hg debugdata .hg/store/data/a.i 1 # verify this is no the file content but includes "oid", the LFS "pointer".
  version https://git-lfs.github.com/spec/v1
  oid sha256:bdc26931acfb734b142a8d675f205becf27560dc461f501822de13274fe6fc8a
  size 6
  x-is-binary 0
  $ cd ..
  $ rm -rf `hg config lfs.usercache`

(Restart the server in a different location so it no longer has the content)

  $ $PYTHON $RUNTESTDIR/killdaemons.py $DAEMON_PIDS
  $ rm $DAEMON_PIDS
  $ mkdir $TESTTMP/lfs-server2
  $ cd $TESTTMP/lfs-server2
#if no-windows
  $ lfs-test-server &> lfs-server.log &
  $ echo $! >> $DAEMON_PIDS
  $ disown %+
#else
  $ $PYTHON $TESTTMP/spawn.py >> $DAEMON_PIDS
#endif

  $ cd $TESTTMP
  $ hg clone test test2
  updating to branch default
  abort: LFS server error. Remote object for file data/a.i not found:(.*)! (re)
  [255]

Adhoc upload and download:

  $ mkdir $TESTTMP/adhoc
  $ cd $TESTTMP/adhoc
  $ echo FOOBAR | hg debuglfssend
  091321a885bbb5bc4c711cfefdeb7697002f83f953e211a87d4e26c6bfcc1825 7

  $ hg debuglfsrecv 091321a885bbb5bc4c711cfefdeb7697002f83f953e211a87d4e26c6bfcc1825 7
  FOOBAR

Clean up:

  $ $PYTHON $RUNTESTDIR/killdaemons.py $DAEMON_PIDS
