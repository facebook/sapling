Create repo with old manifest

  $ cat << EOF >> $HGRCPATH
  > [format]
  > usegeneraldelta=yes
  > EOF

  $ hg init existing
  $ cd existing
  $ echo footext > foo
  $ hg add foo
  $ hg commit -m initial

We're using v1, so no manifestv2 entry is in requires yet.

  $ grep manifestv2 .hg/requires
  [1]

Let's clone this with manifestv2 enabled to switch to the new format for
future commits.

  $ cd ..
  $ hg clone --pull existing new --config experimental.manifestv2=1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd new

Check that entry was added to .hg/requires.

  $ grep manifestv2 .hg/requires
  manifestv2

Make a new commit.

  $ echo newfootext > foo
  $ hg commit -m new

Check that the manifest actually switched to v2.

  $ hg debugdata -m 0
  foo\x0021e958b1dca695a60ee2e9cf151753204ee0f9e9 (esc)

  $ hg debugdata -m 1
  \x00 (esc)
  \x00foo\x00 (esc)
  I\xab\x7f\xb8(\x83\xcas\x15\x9d\xc2\xd3\xd3:5\x08\xbad5_ (esc)

Check that manifestv2 is used if the requirement is present, even if it's
disabled in the config.

  $ echo newerfootext > foo
  $ hg --config experimental.manifestv2=False commit -m newer

  $ hg debugdata -m 2
  \x00 (esc)
  \x00foo\x00 (esc)
  \xa6\xb1\xfb\xef]\x91\xa1\x19`\xf3.#\x90S\xf8\x06 \xe2\x19\x00 (esc)

Check that we can still read v1 manifests.

  $ hg files -r 0
  foo

  $ cd ..

Check that entry is added to .hg/requires on repo creation

  $ hg --config experimental.manifestv2=True init repo
  $ cd repo
  $ grep manifestv2 .hg/requires
  manifestv2

Set up simple repo

  $ echo a > file1
  $ echo b > file2
  $ echo c > file3
  $ hg ci -Aqm 'initial'
  $ echo d > file2
  $ hg ci -m 'modify file2'

Check that 'hg verify', which uses manifest.readdelta(), works

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 2 changesets, 4 total revisions

Check that manifest revlog is smaller than for v1

  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      81     -1       0 57361477c778 000000000000 000000000000
       1        81      33      0       1 aeaab5a2ef74 57361477c778 000000000000
