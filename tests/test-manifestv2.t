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

TODO: Check that manifest revlog is smaller than for v1

  $ hg debugindex -m
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0     106      0       0 f6279f9f8b31 000000000000 000000000000
       1       106      59      0       1 cd20459b75e6 f6279f9f8b31 000000000000
