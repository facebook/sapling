#require no-eden

Test --mutation, -t of `log`.

  $ setconfig diff.git=true
  $ enable tweakdefaults
  $ setconfig tweakdefaults.logdefaultfollow=True

Mutation log is empty without a working copy parent:

  $ newrepo
  $ sl log --mutation

  $ newrepo
  $ drawdag --no-files << 'EOS'
  >   B3  # B3/f=1\n2\n3\n4\n
  >  /    # amend: B1 -> B2 -> B3
  > | B2  # B2/f=1\n2\n3\n
  > |/
  > | B1  # B1/f=1\n2\n
  > |/
  > A     # A/f=1\n
  > EOS

Regular commit graph log:

  $ sl log -Gr: -T '{desc}\n'
  o  B3
  │
  │ x  B2
  ├─╯
  │ x  B1
  ├─╯
  o  A

Mutation graph log:
 
  $ sl go $B3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl log -Gt -T '{desc}\n'
  @  B3
  │
  x  B2
  │
  x  B1
