  $ hg init test
  $ cd test
  $ echo foo>foo
  $ hg commit -Am 1 -d '1 0'
  adding foo
  $ echo bar>bar
  $ hg commit -Am 2 -d '2 0'
  adding bar
  $ mkdir baz
  $ echo bletch>baz/bletch
  $ hg commit -Am 3 -d '1000000000 0'
  adding baz/bletch
  $ echo "[web]" >> .hg/hgrc
  $ echo "name = test-archive" >> .hg/hgrc
  $ cp .hg/hgrc .hg/hgrc-base
  > test_archtype() {
  >     echo "allow_archive = $1" >> .hg/hgrc
  >     hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  >     cat hg.pid >> $DAEMON_PIDS
  >     echo % $1 allowed should give 200
  >     "$TESTDIR/get-with-headers.py" localhost:$HGPORT "/archive/tip.$2" | head -n 1
  >     echo % $3 and $4 disallowed should both give 403
  >     "$TESTDIR/get-with-headers.py" localhost:$HGPORT "/archive/tip.$3" | head -n 1
  >     "$TESTDIR/get-with-headers.py" localhost:$HGPORT "/archive/tip.$4" | head -n 1
  >     "$TESTDIR/killdaemons.py"
  >     cat errors.log
  >     cp .hg/hgrc-base .hg/hgrc
  > }

check http return codes

  $ test_archtype gz tar.gz tar.bz2 zip
  % gz allowed should give 200
  200 Script output follows
  % tar.bz2 and zip disallowed should both give 403
  403 Archive type not allowed: bz2
  403 Archive type not allowed: zip
  $ test_archtype bz2 tar.bz2 zip tar.gz
  % bz2 allowed should give 200
  200 Script output follows
  % zip and tar.gz disallowed should both give 403
  403 Archive type not allowed: zip
  403 Archive type not allowed: gz
  $ test_archtype zip zip tar.gz tar.bz2
  % zip allowed should give 200
  200 Script output follows
  % tar.gz and tar.bz2 disallowed should both give 403
  403 Archive type not allowed: gz
  403 Archive type not allowed: bz2

  $ echo "allow_archive = gz bz2 zip" >> .hg/hgrc
  $ hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

invalid arch type should give 404

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT "/archive/tip.invalid" | head -n 1
  404 Unsupported archive type: None

  $ TIP=`hg id -v | cut -f1 -d' '`
  $ QTIP=`hg id -q`
  $ cat > getarchive.py <<EOF
  > import os, sys, urllib2
  > try:
  >     # Set stdout to binary mode for win32 platforms
  >     import msvcrt
  >     msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
  > except ImportError:
  >     pass
  > node, archive = sys.argv[1:]
  > f = urllib2.urlopen('http://127.0.0.1:%s/?cmd=archive;node=%s;type=%s'
  >                     % (os.environ['HGPORT'], node, archive))
  > sys.stdout.write(f.read())
  > EOF
  $ python getarchive.py "$TIP" gz | gunzip | tar tf - 2>/dev/null
  test-archive-2c0277f05ed4/.hg_archival.txt
  test-archive-2c0277f05ed4/bar
  test-archive-2c0277f05ed4/baz/bletch
  test-archive-2c0277f05ed4/foo
  $ python getarchive.py "$TIP" bz2 | bunzip2 | tar tf - 2>/dev/null
  test-archive-2c0277f05ed4/.hg_archival.txt
  test-archive-2c0277f05ed4/bar
  test-archive-2c0277f05ed4/baz/bletch
  test-archive-2c0277f05ed4/foo
  $ python getarchive.py "$TIP" zip > archive.zip
  $ unzip -t archive.zip
  Archive:  archive.zip
      testing: test-archive-2c0277f05ed4/.hg_archival.txt   OK
      testing: test-archive-2c0277f05ed4/bar   OK
      testing: test-archive-2c0277f05ed4/baz/bletch   OK
      testing: test-archive-2c0277f05ed4/foo   OK
  No errors detected in compressed data of archive.zip.

  $ "$TESTDIR/killdaemons.py"

  $ hg archive -t tar test.tar
  $ tar tf test.tar
  test/.hg_archival.txt
  test/bar
  test/baz/bletch
  test/foo

  $ hg archive -t tbz2 -X baz test.tar.bz2
  $ bunzip2 -dc test.tar.bz2 | tar tf - 2>/dev/null
  test/.hg_archival.txt
  test/bar
  test/foo

  $ hg archive -t tgz -p %b-%h test-%h.tar.gz
  $ gzip -dc test-$QTIP.tar.gz | tar tf - 2>/dev/null
  test-2c0277f05ed4/.hg_archival.txt
  test-2c0277f05ed4/bar
  test-2c0277f05ed4/baz/bletch
  test-2c0277f05ed4/foo

  $ hg archive autodetected_test.tar
  $ tar tf autodetected_test.tar
  autodetected_test/.hg_archival.txt
  autodetected_test/bar
  autodetected_test/baz/bletch
  autodetected_test/foo

The '-t' should override autodetection

  $ hg archive -t tar autodetect_override_test.zip
  $ tar tf autodetect_override_test.zip
  autodetect_override_test.zip/.hg_archival.txt
  autodetect_override_test.zip/bar
  autodetect_override_test.zip/baz/bletch
  autodetect_override_test.zip/foo

  $ for ext in tar tar.gz tgz tar.bz2 tbz2 zip; do
  >     hg archive auto_test.$ext
  >     if [ -d auto_test.$ext ]; then
  >         echo "extension $ext was not autodetected."
  >     fi
  > done

  $ cat > md5comp.py <<EOF
  > try:
  >     from hashlib import md5
  > except ImportError:
  >     from md5 import md5
  > import sys
  > f1, f2 = sys.argv[1:3]
  > h1 = md5(file(f1, 'rb').read()).hexdigest()
  > h2 = md5(file(f2, 'rb').read()).hexdigest()
  > print h1 == h2 or "md5 differ: " + repr((h1, h2))
  > EOF

archive name is stored in the archive, so create similar archives and
rename them afterwards.

  $ hg archive -t tgz tip.tar.gz
  $ mv tip.tar.gz tip1.tar.gz
  $ sleep 1
  $ hg archive -t tgz tip.tar.gz
  $ mv tip.tar.gz tip2.tar.gz
  $ python md5comp.py tip1.tar.gz tip2.tar.gz
  True

  $ hg archive -t zip -p /illegal test.zip
  abort: archive prefix contains illegal components
  [255]
  $ hg archive -t zip -p very/../bad test.zip

  $ hg archive --config ui.archivemeta=false -t zip -r 2 test.zip
  $ unzip -t test.zip
  Archive:  test.zip
      testing: test/bar                 OK
      testing: test/baz/bletch          OK
      testing: test/foo                 OK
  No errors detected in compressed data of test.zip.

  $ hg archive -t tar - | tar tf - 2>/dev/null
  test-2c0277f05ed4/.hg_archival.txt
  test-2c0277f05ed4/bar
  test-2c0277f05ed4/baz/bletch
  test-2c0277f05ed4/foo

  $ hg archive -r 0 -t tar rev-%r.tar
  $ if [ -f rev-0.tar ]; then
  $ fi

test .hg_archival.txt

  $ hg archive ../test-tags
  $ cat ../test-tags/.hg_archival.txt
  repo: daa7f7c60e0a224faa4ff77ca41b2760562af264
  node: 2c0277f05ed49d1c8328fb9ba92fba7a5ebcb33e
  branch: default
  latesttag: null
  latesttagdistance: 3
  $ hg tag -r 2 mytag
  $ hg tag -r 2 anothertag
  $ hg archive -r 2 ../test-lasttag
  $ cat ../test-lasttag/.hg_archival.txt
  repo: daa7f7c60e0a224faa4ff77ca41b2760562af264
  node: 2c0277f05ed49d1c8328fb9ba92fba7a5ebcb33e
  branch: default
  tag: anothertag
  tag: mytag

  $ hg archive -t bogus test.bogus
  abort: unknown archive type 'bogus'
  [255]

enable progress extension:

  $ cp $HGRCPATH $HGRCPATH.no-progress
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > progress =
  > [progress]
  > assume-tty = 1
  > format = topic bar number
  > delay = 0
  > refresh = 0
  > width = 60
  > EOF

  $ hg archive ../with-progress 2>&1 | $TESTDIR/filtercr.py
  
  archiving [                                           ] 0/4
  archiving [                                           ] 0/4
  archiving [=========>                                 ] 1/4
  archiving [=========>                                 ] 1/4
  archiving [====================>                      ] 2/4
  archiving [====================>                      ] 2/4
  archiving [===============================>           ] 3/4
  archiving [===============================>           ] 3/4
  archiving [==========================================>] 4/4
  archiving [==========================================>] 4/4
                                                              \r (esc)

cleanup after progress extension test:

  $ cp $HGRCPATH.no-progress $HGRCPATH

server errors

  $ cat errors.log

empty repo

  $ hg init ../empty
  $ cd ../empty
  $ hg archive ../test-empty
  abort: no working directory: please specify a revision
  [255]

old file -- date clamped to 1980

  $ touch -t 197501010000 old
  $ hg add old
  $ hg commit -m old
  $ hg archive ../old.zip
  $ unzip -l ../old.zip
  Archive:  ../old.zip
  \s*Length.* (re)
  *-----* (glob)
  *147*80*00:00*old/.hg_archival.txt (glob)
  *0*80*00:00*old/old (glob)
  *-----* (glob)
  \s*147\s+2 files (re)
