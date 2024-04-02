#debugruntest-compatible

#require no-eden


  $ configure modernclient
  $ enable rebase smartlog

Prepare repo

  $ newclientrepo
  $ echo 1 >> x
  $ hg ci -Am 'add x'
  adding x
  $ hg mv x y
  $ hg ci -m 'mv x -> y'
  $ cat y
  1
  $ hg prev
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [8cdbe9] add x
  $ echo 2 >> x
  $ hg ci -m 'update x'

  $ hg rebase -s . -d 'desc("mv x -> y")' --config copytrace.dagcopytrace=False
  rebasing d6e243dd00b2 "update x"
  other [source] changed x which local [dest] is missing
  hint: if this is due to a renamed file, you can manually input the renamed path
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg st
  M x
  $ hg resolve --mark x
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg cont
  rebasing d6e243dd00b2 "update x"
