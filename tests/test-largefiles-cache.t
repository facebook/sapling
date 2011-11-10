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
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

Update working directory to "tip", which requires largefile("large"),
but there is no cache file for it.  So, hg must treat it as
"missing"(!) file.

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  large: Can't get file locally
  (no default or default-push path set in hgrc)
  0 largefiles updated, 0 removed
  $ hg status
  ! large

Update working directory to null: this cleanup .hg/largefiles/dirstate

  $ hg update null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  getting changed largefiles
  0 largefiles updated, 0 removed

Update working directory to tip, again.

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  large: Can't get file locally
  (no default or default-push path set in hgrc)
  0 largefiles updated, 0 removed
  $ hg status
  ! large
