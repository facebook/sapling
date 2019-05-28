  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ echo y > y
  $ echo z > z
  $ hg commit -qAm xy
  $ cd ..

  $ cat > cacheprocess-logger.py <<EOF
  > import sys, os, shutil
  > f = open('$TESTTMP/cachelog.log', 'w')
  > srccache = os.path.join('$TESTTMP', 'oldhgcache')
  > def log(message):
  >     f.write(message)
  >     f.flush()
  > destcache = sys.argv[-1]
  > try:
  >     while True:
  >         cmd = sys.stdin.readline().strip()
  >         log('got command %r\n' % cmd)
  >         if cmd == 'exit':
  >             sys.exit(0)
  >         elif cmd == 'get':
  >             count = int(sys.stdin.readline())
  >             log('client wants %r blobs\n' % count)
  >             wants = []
  >             for _ in xrange(count):
  >                 key = sys.stdin.readline()[:-1]
  >                 wants.append(key)
  >                 if '\0' in key:
  >                     _, key = key.split('\0')
  >                 srcpath = os.path.join(srccache, key)
  >                 if os.path.exists(srcpath):
  >                     dest = os.path.join(destcache, key)
  >                     destdir = os.path.dirname(dest)
  >                     if not os.path.exists(destdir):
  >                         os.makedirs(destdir)
  >                     shutil.copyfile(srcpath, dest)
  >                 else:
  >                     # report a cache miss
  >                     sys.stdout.write(key + '\n')
  >             sys.stdout.write('0\n')
  >             for key in sorted(wants):
  >                 log('requested %r\n' % key)
  >             sys.stdout.flush()
  >         elif cmd == 'set':
  >             assert False, 'todo writing'
  >         else:
  >             assert False, 'unknown command! %r' % cmd
  > except Exception as e:
  >     log('Exception! %r\n' % e)
  >     raise
  > EOF

  $ cat >> $HGRCPATH <<EOF
  > [remotefilelog]
  > cacheprocess = python $TESTTMP/cacheprocess-logger.py
  > EOF

Test cache keys and cache misses.
  $ hgcloneshallow ssh://user@dummy/repo clone -q
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)
  $ cat cachelog.log
  got command 'get'
  client wants 3 blobs
  requested 'master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0'
  requested 'master/39/5df8f7c51f007019cb30201c49e884b46b92fa/69a1b67522704ec122181c0890bd16e9d3e7516a'
  requested 'master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca'
  got command 'set'
  Exception! AssertionError('todo writing',)

Test cache hits.
  $ mv hgcache oldhgcache
  $ rm cachelog.log
  $ hgcloneshallow ssh://user@dummy/repo clone-cachehit -q
  3 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over *s (glob)
  $ cat cachelog.log | grep -v exit
  got command 'get'
  client wants 3 blobs
  requested 'master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0'
  requested 'master/39/5df8f7c51f007019cb30201c49e884b46b92fa/69a1b67522704ec122181c0890bd16e9d3e7516a'
  requested 'master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca'


  $ cat >> $HGRCPATH <<EOF
  > [remotefilelog]
  > cacheprocess.includepath = yes
  > EOF

Test cache keys and cache misses with includepath.
  $ rm -r hgcache oldhgcache
  $ rm cachelog.log
  $ hgcloneshallow ssh://user@dummy/repo clone-withpath -q
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)
  $ cat cachelog.log
  got command 'get'
  client wants 3 blobs
  requested 'x\x00master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0'
  requested 'y\x00master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca'
  requested 'z\x00master/39/5df8f7c51f007019cb30201c49e884b46b92fa/69a1b67522704ec122181c0890bd16e9d3e7516a'
  got command 'set'
  Exception! AssertionError('todo writing',)

Test cache hits with includepath.
  $ mv hgcache oldhgcache
  $ rm cachelog.log
  $ hgcloneshallow ssh://user@dummy/repo clone-withpath-cachehit -q
  3 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over *s (glob)
  $ cat cachelog.log | grep -v exit
  got command 'get'
  client wants 3 blobs
  requested 'x\x00master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0'
  requested 'y\x00master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca'
  requested 'z\x00master/39/5df8f7c51f007019cb30201c49e884b46b92fa/69a1b67522704ec122181c0890bd16e9d3e7516a'

Make sure we don't write to the cache in remotefilelog.updatesharedcache=False
  $ rm -r hgcache oldhgcache
  $ rm cachelog.log
  $ hgcloneshallow ssh://user@dummy/repo clone-no-write-to-cache -q --config remotefilelog.updatesharedcache=False
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)
  $ cat cachelog.log
  got command 'get'
  client wants 3 blobs
  requested 'x\x00master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0'
  requested 'y\x00master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca'
  requested 'z\x00master/39/5df8f7c51f007019cb30201c49e884b46b92fa/69a1b67522704ec122181c0890bd16e9d3e7516a'
  got command 'exit'
