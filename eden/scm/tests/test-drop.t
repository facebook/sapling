#require no-eden

#inprocess-hg-incompatible

  $ eagerepo

  $ enable drop

Drop requires the rebase extension:
  $ newclientrepo
  $ sl drop 1
  extension rebase not found
  abort: required extensions not detected
  [255]

  $ cd $TESTTMP
  $ enable rebase

No revision provided:
  $ newclientrepo
  $ sl drop
  abort: no revision to drop was provided
  [255]

Root changesets cannot be dropped:
  $ newclientrepo
  $ drawdag <<'EOS'
  > A
  > EOS
  $ sl drop -r $A
  abort: root changeset cannot be dropped
  [255]

Drop a changeset from the middle of a stack:
  $ newclientrepo
  $ drawdag <<'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl drop -r $C
  Dropping changeset 26805a: C
  rebasing f585351a92f8 "D"
  $ tglog
  o  1e6da8103bc7 'D'
  │
  o  112478962961 'B'
  │
  o  426bada5c675 'A'

Only one revision can be dropped at a time:
  $ sl drop -r $A -r $B
  abort: only one revision can be dropped at a time
  [255]

Drop a changeset with multiple children:
  $ newclientrepo
  $ drawdag <<'EOS'
  > D E
  > |/
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl drop $B
  Dropping changeset 112478: B
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"
  rebasing 78d2dca436b2 "E"
  $ tglog
  o  8b7d4865d115 'E'
  │
  │ o  43a2ce673b14 'D'
  ├─╯
  o  fac0cc90d19e 'C'
  │
  o  426bada5c675 'A'

Merge changesets cannot be dropped:
  $ newclientrepo
  $ drawdag <<'EOS'
  >   D
  >  /|
  > B C
  > |/
  > A
  > EOS
  $ sl drop $D
  abort: merge changeset cannot be dropped
  [255]

Public changesets cannot be dropped:
  $ newclientrepo
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS
  $ sl debugmakepublic $B
  $ sl drop $B
  abort: public changeset which landed cannot be dropped
  [255]

Conflicts while rebasing descendants stop the drop:
  $ newclientrepo
  $ drawdag <<'EOS'
  > C  # C/f = 3\n
  > |
  > B  # B/f = 2\n
  > |
  > A  # A/f = 1\n
  > EOS
  $ sl drop $B
  Dropping changeset d24cfa: B
  rebasing 11b100dbbeb1 "C"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  conflict occurred during drop: please fix it by running 'sl rebase --continue', and then re-run 'sl drop'
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
