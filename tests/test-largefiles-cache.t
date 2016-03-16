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
  $ hg commit -m 'branchhead without largefile' large
  $ hg up -qr 0
  $ rm large
  $ echo "0000000000000000000000000000000000000000" > .hglf/large
  $ hg commit -m 'commit missing file with corrupt standin' large
  abort: large: file not found!
  [255]
  $ hg up -Cqr 0
  $ cd ..

Discard all cached largefiles in USERCACHE

  $ rm -rf ${USERCACHE}

Create mirror repo, and pull from source without largefile:
"pull" is used instead of "clone" for suppression of (1) updating to
tip (= caching largefile from source repo), and (2) recording source
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
  large: largefile 7f7097b041ccf68cc5561e9600da4655d21c6d18 not available from file:/*/$TESTTMP/mirror (glob)
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
  large: largefile 7f7097b041ccf68cc5561e9600da4655d21c6d18 not available from file:/*/$TESTTMP/mirror (glob)
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

Test issue 4053 (remove --after on a deleted, uncommitted file shouldn't say
it is missing, but a remove on a nonexistent unknown file still should.  Same
for a forget.)

  $ cd src
  $ touch x
  $ hg add x
  $ mv x y
  $ hg remove -A x y ENOENT
  ENOENT: * (glob)
  not removing y: file is untracked
  [1]
  $ hg add y
  $ mv y z
  $ hg forget y z ENOENT
  ENOENT: * (glob)
  not removing z: file is already untracked
  [1]

Largefiles are accessible from the share's store
  $ cd ..
  $ hg share -q src share_dst --config extensions.share=
  $ hg -R share_dst update -r0
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo modified > share_dst/large
  $ hg -R share_dst ci -m modified
  created new head

Only dirstate is in the local store for the share, and the largefile is in the
share source's local store.  Avoid the extra largefiles added in the unix
conditional above.
  $ hash=`hg -R share_dst cat share_dst/.hglf/large`
  $ echo $hash
  e2fb5f2139d086ded2cb600d5a91a196e76bf020

  $ find share_dst/.hg/largefiles/* | sort
  share_dst/.hg/largefiles/dirstate

  $ find src/.hg/largefiles/* | egrep "(dirstate|$hash)" | sort
  src/.hg/largefiles/dirstate
  src/.hg/largefiles/e2fb5f2139d086ded2cb600d5a91a196e76bf020

Inject corruption into the largefiles store and see how update handles that:

  $ cd src
  $ hg up -qC tip
  $ cat large
  modified
  $ rm large
  $ cat .hglf/large
  e2fb5f2139d086ded2cb600d5a91a196e76bf020
  $ mv .hg/largefiles/e2fb5f2139d086ded2cb600d5a91a196e76bf020 ..
  $ echo corruption > .hg/largefiles/e2fb5f2139d086ded2cb600d5a91a196e76bf020
  $ hg up -C
  getting changed largefiles
  large: data corruption in $TESTTMP/src/.hg/largefiles/e2fb5f2139d086ded2cb600d5a91a196e76bf020 with hash 6a7bb2556144babe3899b25e5428123735bb1e27 (glob)
  0 largefiles updated, 0 removed
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [12] other heads for branch "default" (re)
  $ hg st
  ! large
  ? z
  $ rm .hg/largefiles/e2fb5f2139d086ded2cb600d5a91a196e76bf020

#if serve

Test coverage of error handling from putlfile:

  $ mkdir $TESTTMP/mirrorcache
  $ hg serve -R ../mirror -d -p $HGPORT1 --pid-file hg.pid --config largefiles.usercache=$TESTTMP/mirrorcache
  $ cat hg.pid >> $DAEMON_PIDS

  $ hg push http://localhost:$HGPORT1 -f --config files.usercache=nocache
  pushing to http://localhost:$HGPORT1/
  searching for changes
  abort: remotestore: could not open file $TESTTMP/src/.hg/largefiles/e2fb5f2139d086ded2cb600d5a91a196e76bf020: HTTP Error 403: ssl required
  [255]

  $ rm .hg/largefiles/e2fb5f2139d086ded2cb600d5a91a196e76bf020

Test coverage of 'missing from store':

  $ hg serve -R ../mirror -d -p $HGPORT2 --pid-file hg.pid --config largefiles.usercache=$TESTTMP/mirrorcache --config "web.allow_push=*" --config web.push_ssl=no
  $ cat hg.pid >> $DAEMON_PIDS

  $ hg push http://localhost:$HGPORT2 -f --config largefiles.usercache=nocache
  pushing to http://localhost:$HGPORT2/
  searching for changes
  abort: largefile e2fb5f2139d086ded2cb600d5a91a196e76bf020 missing from store (needs to be uploaded)
  [255]

Verify that --lfrev controls which revisions are checked for largefiles to push

  $ hg push http://localhost:$HGPORT2 -f --config largefiles.usercache=nocache --lfrev tip
  pushing to http://localhost:$HGPORT2/
  searching for changes
  abort: largefile e2fb5f2139d086ded2cb600d5a91a196e76bf020 missing from store (needs to be uploaded)
  [255]

  $ hg push http://localhost:$HGPORT2 -f --config largefiles.usercache=nocache --lfrev null
  pushing to http://localhost:$HGPORT2/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files (+1 heads)

#endif
