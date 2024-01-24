#debugruntest-compatible

  $ configure modern
  $ enable rebase
  $ setconfig automerge.merge-algos=adjacent-changes,subset-changes
  $ setconfig automerge.disable-for-noninteractive=False

Successful adjacent-changes merge:

  $ newrepo
  $ setconfig automerge.mode=accept
  $ drawdag <<'EOS'
  > B C # C/A=a\nb\nc'\nd'\ne\n
  > |/  # B/A=a\nb'\nc\nd\ne\n
  > A   # A/A=a\nb\nc\nd\ne\n
  > EOS
  $ hg rebase -r $C -d $B
  rebasing 929db2f4565d "C"
  merging A
   lines 2-4 have been resolved by automerge algorithms
  $ hg cat -r tip A
  a
  b'
  c'
  d'
  e

adjacent-changes merge - prompt:

  $ newrepo
  $ setconfig automerge.mode=prompt
  $ drawdag <<'EOS'
  > B C # C/A=a\nb\nc'\nd'\ne\n
  > |/  # B/A=a\nb'\nc\nd\ne\n
  > A   # A/A=a\nb\nc\nd\ne\n
  > EOS
  $ hg rebase -r $C -d $B
  rebasing 929db2f4565d "C"
  merging A
  <<<<<<< dest:   6d1bb9f58190 - test: B
  -b
  +b'
   c
   d
  ======= base:   98e058757f9d - test: A
   b
  -c
  -d
  +c'
  +d'
  >>>>>>> source: 929db2f4565d - test: C
  
  Above conflict can be resolved automatically (see 'hg help automerge' for details):
  <<<<<<< automerge algorithm yields:
   b'
   c'
   d'
  >>>>>>>
  Accept this resolution?
  (a)ccept it, (r)eject it, or review it in (f)ile: r
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat A
  a
  <<<<<<< dest:   6d1bb9f58190 - test: B
  b'
  c
  d
  =======
  b
  c'
  d'
  >>>>>>> source: 929db2f4565d - test: C
  e

adjacent-changes merge - keep-in-file:

  $ newrepo
  $ setconfig automerge.mode=review-in-file
  $ drawdag <<'EOS'
  > B C # C/A=a\nb\nc'\nd'\ne\n
  > |/  # B/A=a\nb'\nc\nd\ne\n
  > A   # A/A=a\nb\nc\nd\ne\n
  > EOS
  $ hg rebase -r $C -d $B
  rebasing 929db2f4565d "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat A
  a
  <<<<<<< dest:   6d1bb9f58190 - test: B
  b'
  c
  d
  =======
  b
  c'
  d'
  >>>>>>> source: 929db2f4565d - test: C
  <<<<<<< 'adjacent-changes' automerge algorithm yields:
  b'
  c'
  d'
  >>>>>>>
  e

adjacent-changes merge - disable for noninteractive:

  $ newrepo
  $ setconfig automerge.mode=accept automerge.disable-for-noninteractive=True
  $ drawdag <<'EOS'
  > B C # C/A=a\nb\nc'\nd'\ne\n
  > |/  # B/A=a\nb'\nc\nd\ne\n
  > A   # A/A=a\nb\nc\nd\ne\n
  > EOS
  $ hg rebase -r $C -d $B -q
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Unsuccessful adjacent-changes merge - overlap:

  $ newrepo
  $ setconfig automerge.mode=accept
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
  $ setconfig automerge.mode=accept
  $ drawdag <<'EOS'
  > B C # C/A=a\nb\nd\n
  > |/  # B/A=a\nb\nc\nd\n
  > A   # A/A=a\nd\n
  > EOS
  $ hg rebase -r $C -d $B
  rebasing 58aa52a4f6bb "C"
  merging A
   lines 2-3 have been resolved by automerge algorithms
  $ hg cat -r tip A
  a
  b
  c
  d

adjacent-changes merge - (keep-in-file & merge3):

  $ newrepo
  $ setconfig automerge.mode=review-in-file
  $ drawdag <<'EOS'
  > B C # C/A=a\nb\nc'\nd'\ne\n
  > |/  # B/A=a\nb'\nc\nd\ne\n
  > A   # A/A=a\nb\nc\nd\ne\n
  > EOS
  $ hg rebase -r $C -d $B -q -t internal:merge3
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat A
  a
  <<<<<<< dest:   6d1bb9f58190 - test: B
  b'
  c
  d
  ||||||| base:   98e058757f9d - test: A
  b
  c
  d
  =======
  b
  c'
  d'
  >>>>>>> source: 929db2f4565d - test: C
  <<<<<<< 'adjacent-changes' automerge algorithm yields:
  b'
  c'
  d'
  >>>>>>>
  e

adjacent-changes merge - (keep-in-file & mergediff):

  $ newrepo
  $ setconfig automerge.mode=review-in-file
  $ drawdag <<'EOS'
  > B C # C/A=a\nb\nc'\nd'\ne\n
  > |/  # B/A=a\nb'\nc\nd\ne\n
  > A   # A/A=a\nb\nc\nd\ne\n
  > EOS
  $ hg rebase -r $C -d $B -q -t internal:mergediff
  warning: conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat A
  a
  <<<<<<<
  ------- base:   98e058757f9d - test: A
  +++++++ dest:   6d1bb9f58190 - test: B
  -b
  +b'
   c
   d
  ======= source: 929db2f4565d - test: C
  b
  c'
  d'
  >>>>>>>
  <<<<<<< 'adjacent-changes' automerge algorithm yields:
  b'
  c'
  d'
  >>>>>>>
  e
