  $ enable amend rebase


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
FIXME: this is unexpected - "unamend" reset backwards across rebase
  $ hg unamend
  $ hg st
  R A
