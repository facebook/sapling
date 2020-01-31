#chg-compatible

  $ disable treemanifest
#require unix-permissions no-root

  $ hg init t
  $ cd t

  $ echo foo > a
  $ hg add a

  $ hg commit -m "1"

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

  $ chmod -r .hg/store/data/a.i

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  abort: Permission denied: $TESTTMP/t/.hg/store/data/a.i
  (current process runs with uid 42)
  ($TESTTMP/t/.hg/store/data/a.i: mode 0o52, uid 42, gid 42)
  ($TESTTMP/t/.hg/store/data: mode 0o52, uid 42, gid 42)
  [255]

  $ chmod +r .hg/store/data/a.i

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

  $ chmod -w .hg/store/data/a.i

  $ echo barber > a
  $ hg commit -m "2"
  trouble committing a!
  abort: Permission denied: $TESTTMP/t/.hg/store/data/a.i
  (current process runs with uid 42)
  ($TESTTMP/t/.hg/store/data/a.i: mode 0o52, uid 42, gid 42)
  ($TESTTMP/t/.hg/store/data: mode 0o52, uid 42, gid 42)
  [255]

  $ chmod -w .

  $ hg diff --nodates
  diff -r 2a18120dc1c9 a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -foo
  +barber

  $ chmod +w .

  $ chmod +w .hg/store/data/a.i
  $ mkdir dir
  $ touch dir/a
  $ hg status
  M a
  ? dir/a
  $ chmod -rx dir

#if no-fsmonitor

(fsmonitor makes "hg status" avoid accessing to "dir")

  $ hg status
  dir: Permission denied
  M a

#endif

Reenable perm to allow deletion:

  $ chmod +rx dir

  $ cd ..
