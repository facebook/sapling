
#require no-eden

#testcases tree-store tree-manifest

#if tree-manifest
  $ setconfig experimental.pathhistory-treemanifest=true
#endif

  $ enable rebase

Prepare a repo

  $ newrepo
  $ drawdag <<'EOS'
  > C   # C/y = 1\n (renamed from x)
  > |
  > | D # D/x = 1\n2\n3\n
  > | |
  > | B # B/x = 1\n2\n
  > |/
  > A   # A/x = 1\n
  > EOS

Rebase should succeed

  $ sl rebase -s $B -d $C
  rebasing 4b097f0fb1bf "B"
  merging y and x to y
  rebasing 0918b4413bb6 "D"
  merging y and x to y
