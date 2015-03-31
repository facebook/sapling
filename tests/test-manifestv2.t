Check that entry is added to .hg/requires

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
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0      81      0       0 57361477c778 000000000000 000000000000
       1        81      33      0       1 aeaab5a2ef74 57361477c778 000000000000
