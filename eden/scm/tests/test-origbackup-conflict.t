
#require no-eden


  $ eagerepo
  $ setconfig commands.update.check=none
Set up repo

  $ setconfig ui.origbackuppath=.hg/origbackups merge.checkunknown=warn
  $ sl init repo
  $ cd repo
  $ echo base > base
  $ sl add base
  $ sl commit -m "base"

Make a dir named b that contains a file, and a file named d

  $ mkdir -p b
  $ echo c1 > b/c
  $ echo d1 > d
  $ sl add b/c d
  $ sl commit -m "c1"
  $ sl bookmark c1

Peform an update that causes b/c to be backed up

  $ sl up -q 'desc(base)'
  $ mkdir -p b
  $ echo c2 > b/c
  $ sl up --verbose c1
  resolving manifests
  b/c: replacing untracked file
  getting b/c
  creating directory: $TESTTMP/repo/.sl/origbackups/b
  getting d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark c1)
  $ test -f .sl/origbackups/b/c

Make files named b and d

  $ sl up -q 'desc(base)'
  $ echo b1 > b
  $ echo d2 > d
  $ sl add b d
  $ sl commit -m b1
  $ sl bookmark b1

Perform an update that causes b to be backed up - it should replace the backup b dir

  $ sl up -q 'desc(base)'
  $ echo b2 > b
  $ sl up --verbose b1
  resolving manifests
  b: replacing untracked file
  getting b
  removing conflicting directory: $TESTTMP/repo/.sl/origbackups/b
  getting d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark b1)
  $ test -f .sl/origbackups/b

Perform an update the causes b/c to be backed up again - it should replace the backup b file

  $ sl up -q 'desc(base)'
  $ mkdir b
  $ echo c3 > b/c
  $ sl up --verbose c1
  resolving manifests
  b/c: replacing untracked file
  getting b/c
  creating directory: $TESTTMP/repo/.sl/origbackups/b
  removing conflicting file: $TESTTMP/repo/.sl/origbackups/b
  getting d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark c1)
  $ test -d .sl/origbackups/b

Cause two symlinks to be backed up that points to a valid location from the backup dir

  $ sl up -q 'desc(base)'
  $ mkdir ../sym-link-target
#if symlink
  $ ln -s ../../../sym-link-target b
  $ ln -s ../../../sym-link-target d
#else
  $ touch b d
#endif
  $ sl up b1
  b: replacing untracked file
  d: replacing untracked file
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark b1)
#if symlink
  $ f .sl/origbackups/b
  .sl/origbackups/b -> ../../../sym-link-target
#endif

Perform an update that causes b/c and d to be backed up again - b/c should not go into the target dir

  $ sl up -q 'desc(base)'
  $ mkdir b
  $ echo c4 > b/c
  $ echo d3 > d
  $ sl up --verbose c1
  resolving manifests
  b/c: replacing untracked file
  d: replacing untracked file
  getting b/c
  creating directory: $TESTTMP/repo/.sl/origbackups/b
  removing conflicting file: $TESTTMP/repo/.sl/origbackups/b
  getting d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark c1)
  $ cat .sl/origbackups/b/c
  c4
  $ cat .sl/origbackups/d
  d3
  $ ls ../sym-link-target

Incorrectly configure origbackuppath to be under a file

#if no-windows
Does not work on Windows: abort: The process cannot access the file because it is being used by another process: $TESTTMP\repo\.sl/badorigbackups

  $ echo data > .sl/badorigbackups
  $ sl up -q 'desc(base)'
  $ mkdir b
  $ echo c5 > b/c
  $ sl up --verbose c1 --config ui.origbackuppath=.hg/badorigbackups
  resolving manifests
  b/c: replacing untracked file
  getting b/c
  creating directory: $TESTTMP/repo/.sl/badorigbackups/b
  removing conflicting file: $TESTTMP/repo/.sl/badorigbackups
  getting d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark c1)
  $ ls .sl/badorigbackups/b
  c
#endif
