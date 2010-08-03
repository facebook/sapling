Create a repository:

  $ mkdir t
  $ cd t
  $ hg init

Make a changeset:

  $ echo a > a
  $ hg add a
  $ hg commit -m test -d "1000000 0"

This command is ancient:

  $ hg history
  changeset:   0:0acdaf898367
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     test
  

Poke around at hashes:

  $ hg manifest --debug
  b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3 644   a

  $ hg cat a
  a

Verify should succeed:

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

At the end...
