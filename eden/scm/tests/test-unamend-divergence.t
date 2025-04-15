  $ enable amend rebase undo

  $ newclientrepo
  $ drawdag <<EOS
  > B   C
  >  \ /
  >   A
  > EOS
  $ hg go -q $C
  $ echo changed > C
  $ hg amend -q
  $ hg rebase -qr . -d $B
  $ hg unamend
  abort: commit was not amended
  (use "hg undo" to undo the last command, or "hg reset COMMIT" to reset to a previous commit, or see "hg journal" to view commit mutations)
  [255]
  $ hg undo -q
  $ hg unamend
  $ hg st
  M C
