Create user cache directory

  $ USERCACHE=`pwd`/cache; export USERCACHE
  $ cat <<EOF >> ${HGRCPATH}
  > [extensions]
  > hgext.largefiles=
  > [largefiles]
  > usercache=${USERCACHE}
  > EOF
  $ mkdir -p ${USERCACHE}

Create source repo, and commit adding largefile.

  $ hg init src
  $ cd src
  $ echo large > large
  $ hg add --large large
  $ hg commit -m 'add largefile'
  $ hg rm large
  $ hg commit -m 'branchhead without largefile'
  $ hg up -qr 0
  $ cd ..

Discard all cached largefiles in USERCACHE

  $ rm -rf ${USERCACHE}

Create mirror repo, and pull from source without largefile:
"pull" is used instead of "clone" for suppression of (1) updating to
tip (= cahcing largefile from source repo), and (2) recording source
repo as "default" path in .hg/hgrc.

  $ hg init mirror
  $ cd mirror
  $ hg pull ../src
  pulling from ../src
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

Update working directory to "tip", which requires largefile("large"),
but there is no cache file for it.  So, hg must treat it as
"missing"(!) file.

  $ hg update -r0
  getting changed largefiles
  large: largefile 7f7097b041ccf68cc5561e9600da4655d21c6d18 not available from file://$TESTTMP/mirror (glob)
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  ! large

Update working directory to null: this cleanup .hg/largefiles/dirstate

  $ hg update null
  getting changed largefiles
  0 largefiles updated, 0 removed
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Update working directory to tip, again.

  $ hg update -r0
  getting changed largefiles
  large: largefile 7f7097b041ccf68cc5561e9600da4655d21c6d18 not available from file://$TESTTMP/mirror (glob)
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  ! large
  $ cd ..

Verify that largefiles from pulled branchheads are fetched, also to an empty repo

  $ hg init mirror2
  $ hg -R mirror2 pull src -r0
  pulling from src
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

#if unix-permissions

Portable way to print file permissions:

  $ cat > ls-l.py <<EOF
  > #!/usr/bin/env python
  > import sys, os
  > path = sys.argv[1]
  > print '%03o' % (os.lstat(path).st_mode & 0777)
  > EOF
  $ chmod +x ls-l.py

Test that files in .hg/largefiles inherit mode from .hg/store, not
from file in working copy:

  $ cd src
  $ chmod 750 .hg/store
  $ chmod 660 large
  $ echo change >> large
  $ hg commit -m change
  created new head
  $ ../ls-l.py .hg/largefiles/e151b474069de4ca6898f67ce2f2a7263adf8fea
  640

Test permission of with files in .hg/largefiles created by update:

  $ cd ../mirror
  $ rm -r "$USERCACHE" .hg/largefiles # avoid links
  $ chmod 750 .hg/store
  $ hg pull ../src --update -q
  $ ../ls-l.py .hg/largefiles/e151b474069de4ca6898f67ce2f2a7263adf8fea
  640

Test permission of files created by push:

  $ hg serve -R ../src -d -p $HGPORT --pid-file hg.pid \
  >          --config "web.allow_push=*" --config web.push_ssl=no
  $ cat hg.pid >> $DAEMON_PIDS

  $ echo change >> large
  $ hg commit -m change

  $ rm -r "$USERCACHE"

  $ hg push -q http://localhost:$HGPORT/

  $ ../ls-l.py ../src/.hg/largefiles/b734e14a0971e370408ab9bce8d56d8485e368a9
  640

  $ cd ..

#endif
