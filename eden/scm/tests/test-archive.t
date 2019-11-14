#require serve

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

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > share =
  > EOF

  $ echo "[web]" >> .hg/hgrc
  $ echo "name = test-archive" >> .hg/hgrc
  $ cp .hg/hgrc .hg/hgrc-base
  > test_archtype() {
  >     echo "allow_archive = $1" >> .hg/hgrc
  >     test_archtype_run "$@"
  > }
  > test_archtype_deprecated() {
  >     echo "allow$1 = True" >> .hg/hgrc
  >     test_archtype_run "$@"
  > }
  > test_archtype_run() {
  >     hg serve -p 0 --port-file $TESTTMP/.p -d --pid-file=hg.pid -E errors.log
  >     HGPORT=`cat $TESTTMP/.p`
  >     cat hg.pid >> $DAEMON_PIDS
  >     echo % $1 allowed should give 200
  >     get-with-headers.py localhost:$HGPORT "archive/tip.$2" | head -n 1
  >     echo % $3 and $4 disallowed should both give 403
  >     get-with-headers.py localhost:$HGPORT "archive/tip.$3" | head -n 1
  >     get-with-headers.py localhost:$HGPORT "archive/tip.$4" | head -n 1
  >     killdaemons.py
  >     cat errors.log
  >     cp .hg/hgrc-base .hg/hgrc
  > }

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
  > from __future__ import absolute_import, print_function
  > import hashlib
  > import sys
  > f1, f2 = sys.argv[1:3]
  > h1 = hashlib.md5(open(f1, 'rb').read()).hexdigest()
  > h2 = hashlib.md5(open(f2, 'rb').read()).hexdigest()
  > print(h1 == h2 or "md5 differ: " + repr((h1, h2)))
  > EOF

archive name is stored in the archive, so create similar archives and
rename them afterwards.

  $ hg archive -t tgz tip.tar.gz
  $ mv tip.tar.gz tip1.tar.gz
  $ sleep 1
  $ hg archive -t tgz tip.tar.gz
  $ mv tip.tar.gz tip2.tar.gz
  $ $PYTHON md5comp.py tip1.tar.gz tip2.tar.gz
  True

  $ hg archive -t zip -p /illegal test.zip
  abort: archive prefix contains illegal components
  [255]
  $ hg archive -t zip -p very/../bad test.zip

  $ hg archive --config ui.archivemeta=false -t zip -r 2 test.zip
  $ unzip -t test.zip
  Archive:  test.zip
      testing: test/bar*OK (glob)
      testing: test/baz/bletch*OK (glob)
      testing: test/foo*OK (glob)
  No errors detected in compressed data of test.zip.

  $ hg archive -t tar - | tar tf - 2>/dev/null
  test-2c0277f05ed4/.hg_archival.txt
  test-2c0277f05ed4/bar
  test-2c0277f05ed4/baz/bletch
  test-2c0277f05ed4/foo

  $ hg archive -r 0 -t tar rev-%r.tar
  $ [ -f rev-0.tar ]

test .hg_archival.txt

  $ hg archive ../test-tags
  $ cat ../test-tags/.hg_archival.txt
  repo: daa7f7c60e0a224faa4ff77ca41b2760562af264
  node: 2c0277f05ed49d1c8328fb9ba92fba7a5ebcb33e
  branch: default
  latesttag: null
  latesttagdistance: 3
  changessincelatesttag: 3
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
  $ unzip -l ../old.zip | grep -v -- ----- | egrep -v files$
  Archive:  ../old.zip
  \s*Length.* (re)
  *172*80*00:00*old/.hg_archival.txt (glob)
  *0*80*00:00*old/old (glob)

show an error when a provided pattern matches no files

  $ hg archive -I file_that_does_not_exist.foo ../empty.zip
  abort: no files match the archive pattern
  [255]

  $ hg archive -X * ../empty.zip
  abort: no files match the archive pattern
  [255]

  $ cd ..

issue3600: check whether "hg archive" can create archive files which
are extracted with expected timestamp, even though TZ is not
configured as GMT.

  $ mkdir issue3600
  $ cd issue3600

  $ hg init repo
  $ echo a > repo/a
  $ hg -R repo add repo/a
  $ hg -R repo commit -m '#0' -d '456789012 21600'
  $ cat > show_mtime.py <<EOF
  > from __future__ import absolute_import, print_function
  > import os
  > import sys
  > print(int(os.stat(sys.argv[1]).st_mtime))
  > EOF

  $ hg -R repo archive --prefix tar-extracted archive.tar
  $ (TZ=UTC-3; export TZ; tar xf archive.tar)
  $ $PYTHON show_mtime.py tar-extracted/a
  456789012

  $ hg -R repo archive --prefix zip-extracted archive.zip
  $ (TZ=UTC-3; export TZ; unzip -q archive.zip)
  $ $PYTHON show_mtime.py zip-extracted/a
  456789012

  $ cd ..
