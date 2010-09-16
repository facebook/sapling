Create an empty repo:

  $ hg init a
  $ cd a

Try some commands:

  $ hg log
  $ hg grep wah
  [1]
  $ hg manifest
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  0 files, 0 changesets, 0 total revisions

Check the basic files created:

  $ ls .hg
  00changelog.i
  requires
  store

Should be empty:

  $ ls .hg/store

Poke at a clone:

  $ cd ..
  $ hg clone a b
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd b
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  0 files, 0 changesets, 0 total revisions
  $ ls .hg
  00changelog.i
  branch
  dirstate
  hgrc
  requires
  store

Should be empty:

  $ ls .hg/store
