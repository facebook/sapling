
#require no-eden

Test that `sl commit` refuses to create merge commits when ui.allowmerge=false.
This closes the backdoor where `sl commit` creates a merge commit from an
interrupted rebase, even though `sl merge` is blocked.

  $ eagerepo
  $ setconfig ui.allowmerge=false

Setup: create a repo with two branches that will conflict

  $ sl init repo
  $ cd repo
  $ echo base > file.txt
  $ sl add file.txt
  $ sl commit -m "base"

  $ echo branch1 > file.txt
  $ sl commit -m "branch1"

  $ sl goto -C 'desc(base)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo branch2 > file.txt
  $ sl commit -m "branch2"

sl merge is blocked by ui.allowmerge=false

  $ sl merge 'desc(branch1)'
  abort: merging is not supported for this repository
  (use rebase, or 'sl merge --noconflict' for a conflict-free merge)
  [255]

Simulate an interrupted rebase leaving merge state (set p2 directly via debugsetparents)

  $ sl debugsetparents . 'desc(branch1)'

sl commit is now also blocked when p2 is set and ui.allowmerge=false

  $ sl commit -m "should fail"
  abort: working copy has two parents (merge state) - refusing to create a merge commit
  [255]

When ui.allowmerge is true (default), sl commit allows merge commits

  $ sl goto -C 'desc(branch2)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl debugsetparents . 'desc(branch1)'
  $ setconfig ui.allowmerge=true
  $ sl commit -m "merge allowed by config"

  $ cd ..
