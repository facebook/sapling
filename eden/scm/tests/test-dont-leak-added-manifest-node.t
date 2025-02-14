  $ newclientrepo <<EOS
  > B # B/A = (removed)
  > |
  > A
  > EOS
  $ hg go -q $B
  $ touch A
  $ hg add A
  $ LOG=eagerepo::api=debug hg revert -r .^ A 2>&1 | grep history
  DEBUG eagerepo::api: history 005d992c5dcf32993668f7cede29d296c494a5d9
