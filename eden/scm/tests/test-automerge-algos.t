#debugruntest-compatible

  $ configure modern
  $ enable rebase
  $ setconfig automerge.merge-algos=adjacent-changes,subset-changes

Successful adjacent-changes merge:

  $ newrepo
  $ drawdag <<'EOS'
  > B C # C/A=a\nb\nc'\nd'\ne\n
  > |/  # B/A=a\nb'\nc\nd\ne\n
  > A   # A/A=a\nb\nc\nd\ne\n
  > EOS
  $ hg rebase -r $C -d $B -q
  $ hg cat -r tip A
  a
  b'
  c'
  d'
  e

Unsuccessful adjacent-changes merge - overlap:

  $ newrepo
  $ drawdag <<'EOS'
  > B C # C/A=a\nb'\ne\n
  > |/  # B/A=a\na2\nb\nc\nd\ne\n
  > A   # A/A=a\nb\ne\n
  > EOS
  $ hg rebase -r $C -d $B -q
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat A
  a
  <<<<<<< dest:   d2736b3284d1 - test: B
  a2
  b
  c
  d
  =======
  b'
  >>>>>>> source: a2c2f719de49 - test: C
  e

Successful subset-changes merge:

  $ newrepo
  $ drawdag <<'EOS'
  > B C # C/A=a\nb\nd\n
  > |/  # B/A=a\nb\nc\nd\n
  > A   # A/A=a\nd\n
  > EOS
  $ hg rebase -r $C -d $B -q
  $ hg cat -r tip A
  a
  b
  c
  d
