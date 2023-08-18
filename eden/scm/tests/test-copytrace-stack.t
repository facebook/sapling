#debugruntest-compatible
  $ enable rebase copytrace
  $ setconfig experimental.copytrace=off
  $ setconfig copytrace.dagcopytrace=True

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

Rebase should succeed (tofix:zhaolong)

  $ hg rebase -s $B -d $C
  rebasing 4b097f0fb1bf "B"
  merging y and x to y
  rebasing 0918b4413bb6 "D"
  other [source] changed x which local [dest] is missing
  hint: if this is due to a renamed file, you can manually input the renamed path
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
