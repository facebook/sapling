Set up repo

  $ cat << EOF >> $HGRCPATH
  > [ui]
  > origbackuppath=.hg/origbackups
  > [merge]
  > checkunknown=warn
  > EOF
  $ hg init repo
  $ cd repo
  $ echo base > base
  $ hg add base
  $ hg commit -m "base"

Make a dir named b that contains a file, and a file named d

  $ mkdir -p b
  $ echo c1 > b/c
  $ echo d1 > d
  $ hg add b/c d
  $ hg commit -m "c1"
  $ hg bookmark c1

Peform an update that causes b/c to be backed up

  $ hg up -q 0
  $ mkdir -p b
  $ echo c2 > b/c
  $ hg up --verbose c1
  resolving manifests
  b/c: replacing untracked file
  getting b/c
  creating directory: $TESTTMP/repo/.hg/origbackups/b (glob)
  getting d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark c1)
  $ test -f .hg/origbackups/b/c

Make files named b and d

  $ hg up -q 0
  $ echo b1 > b
  $ echo d2 > d
  $ hg add b d
  $ hg commit -m b1
  created new head
  $ hg bookmark b1

Perform an update that causes b to be backed up - it should replace the backup b dir

  $ hg up -q 0
  $ echo b2 > b
  $ hg up --verbose b1
  resolving manifests
  b: replacing untracked file
  getting b
  removing conflicting directory: $TESTTMP/repo/.hg/origbackups/b (glob)
  getting d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark b1)
  $ test -f .hg/origbackups/b

Perform an update the causes b/c to be backed up again - it should replace the backup b file

  $ hg up -q 0
  $ mkdir b
  $ echo c3 > b/c
  $ hg up --verbose c1
  resolving manifests
  b/c: replacing untracked file
  getting b/c
  creating directory: $TESTTMP/repo/.hg/origbackups/b (glob)
  removing conflicting file: $TESTTMP/repo/.hg/origbackups/b (glob)
  getting d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark c1)
  $ test -d .hg/origbackups/b

Cause two symlinks to be backed up that points to a valid location from the backup dir

  $ hg up -q 0
  $ mkdir ../sym-link-target
#if symlink
  $ ln -s ../../../sym-link-target b
  $ ln -s ../../../sym-link-target d
#else
  $ touch b d
#endif
  $ hg up b1
  b: replacing untracked file
  d: replacing untracked file
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark b1)
#if symlink
  $ readlink.py .hg/origbackups/b
  .hg/origbackups/b -> ../../../sym-link-target
#endif

Perform an update that causes b/c and d to be backed up again - b/c should not go into the target dir

  $ hg up -q 0
  $ mkdir b
  $ echo c4 > b/c
  $ echo d3 > d
  $ hg up --verbose c1
  resolving manifests
  b/c: replacing untracked file
  d: replacing untracked file
  getting b/c
  creating directory: $TESTTMP/repo/.hg/origbackups/b (glob)
  removing conflicting file: $TESTTMP/repo/.hg/origbackups/b (glob)
  getting d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark c1)
  $ cat .hg/origbackups/b/c
  c4
  $ cat .hg/origbackups/d
  d3
  $ ls ../sym-link-target

Incorrectly configure origbackuppath to be under a file

  $ echo data > .hg/badorigbackups
  $ hg up -q 0
  $ mkdir b
  $ echo c5 > b/c
  $ hg up --verbose c1 --config ui.origbackuppath=.hg/badorigbackups
  resolving manifests
  b/c: replacing untracked file
  getting b/c
  creating directory: $TESTTMP/repo/.hg/badorigbackups/b (glob)
  abort: The system cannot find the path specified: '$TESTTMP/repo/.hg/badorigbackups/b' (glob) (windows !)
  abort: Not a directory: '$TESTTMP/repo/.hg/badorigbackups/b' (no-windows !)
  [255]
  $ cat .hg/badorigbackups
  data

