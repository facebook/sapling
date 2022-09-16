  $ configure modern

  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

Without revf64compat, rev is not in f64 safe range:

  $ setconfig experimental.revf64compat=0
  $ hg log -r $A -T '{rev}\n'
  72057594037927936

With revf64compat, rev is mapped to f64 safe range:

  $ setconfig experimental.revf64compat=1
  $ hg log -r $B -T '{rev}\n'
  281474976710657
  $ hg log -r $B -T json | grep rev
    "rev": 281474976710657,
  $ hg log -Gr $B -T '{rev}\n'
  o  281474976710657
  │
  ~
  $ hg log -Gr $B -T json | grep rev
  ~    "rev": 281474976710657,
  $ hg tip -T '{rev}\n'
  281474976710657
  $ hg tip -Tjson | grep rev
    "rev": 281474976710657,

Both the original and the mapped revs can be resolved just fine:

  $ hg log -r 72057594037927936+281474976710657 -T '{desc}\n'
  A
  B

The pattern "ifcontains(rev, revset('.'), ...)" can still be used:

  $ hg up -q $B
  $ hg log -r . -T "{ifcontains(rev, revset('.'), '@', 'o')}\n"
  @
