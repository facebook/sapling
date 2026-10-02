
  $ eagerepo
  $ enable rebase drop

No revision provided:
  $ newclientrepo
  $ sl drop
  abort: no revision to drop was provided
  [255]

Revisions must be given with -r:
  $ sl drop 'desc(A)' 'desc(B)'
  abort: revisions to drop must be given with -r
  (use 'sl drop -r desc(A) -r desc(B)')
  [255]

Root changesets cannot be dropped:
  $ newclientrepo
  $ drawdag <<'EOS'
  > A
  > EOS
  $ sl drop -r $A
  abort: root changeset cannot be dropped: 426bada5c675
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
  dropping changeset 26805a: C
  rebasing f585351a92f8 "D"
  $ tglog
  o  1e6da8103bc7 'D'
  │
  o  112478962961 'B'
  │
  o  426bada5c675 'A'

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
  $ sl drop -r $B
  dropping changeset 112478: B
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
  $ sl drop -r $D
  abort: merge changeset cannot be dropped: 4e4f9194f9f1
  [255]

Public changesets cannot be dropped:
  $ newclientrepo
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS
  $ sl debugmakepublic $B
  $ sl drop -r $B
  abort: public changeset cannot be dropped: 112478962961
  [255]

Drop multiple changesets; bookmarks on dropped changesets move to the nearest
kept ancestor:
  $ newclientrepo
  $ drawdag <<'EOS'
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl bookmark -r $D book
  $ sl drop -r $B -r $D
  dropping changeset 112478: B
  dropping changeset f58535 (book): D
  rebasing 26805aba1e60 "C"
  rebasing 9bc730a19041 "E"
  $ tglog
  o  79a39ef1295b 'E'
  │
  o  fac0cc90d19e 'C' book
  │
  o  426bada5c675 'A'

Drop a head changeset:
  $ newclientrepo
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS
  $ sl drop -r $B
  dropping changeset 112478: B
  $ tglog
  o  426bada5c675 'A'

Dropping the working copy parent moves the working copy:
  $ newclientrepo
  $ drawdag <<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $B
  $ sl drop -r .
  dropping changeset 112478: B
  rebasing 26805aba1e60 "C"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 426bada5c675
  $ tglog
  o  fac0cc90d19e 'C'
  │
  @  426bada5c675 'A'

Conflicts while rebasing descendants stop the drop:
  $ newclientrepo
  $ drawdag <<'EOS'
  > D
  > |
  > C  # C/f = 3\n
  > |
  > B  # B/f = 2\n
  > |
  > A  # A/f = 1\n
  > EOS
  $ sl drop -r $B
  dropping changeset d24cfa: B
  rebasing 11b100dbbeb1 "C"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  conflict occurred during drop: please fix it by running 'sl rebase --continue', and then re-run 'sl drop -r d24cfa11bac1'
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]

After resolving the conflict and continuing the rebase, re-running the drop
hides the dropped changeset:
  $ sl resolve --tool internal:other --all
  (no more unresolved files)
  continue: sl rebase --continue
  $ sl rebase --continue
  rebasing 11b100dbbeb1 "C"
  rebasing 7abc7f013e72 "D"
  $ sl drop -r $B
  dropping changeset d24cfa: B
  $ tglog
  o  794127ad5e0d 'D'
  │
  o  b7f0d49ae613 'C'
  │
  o  ac36a1f9437e 'A'

A merge tool can be given with --tool:
  $ newclientrepo
  $ drawdag <<'EOS'
  > C  # C/f = 3\n
  > |
  > B  # B/f = 2\n
  > |
  > A  # A/f = 1\n
  > EOS
  $ sl drop -r $B --tool internal:other
  dropping changeset d24cfa: B
  rebasing 11b100dbbeb1 "C"
  $ sl cat -r 'desc(C)' f
  3
  $ tglog
  o  b7f0d49ae613 'C'
  │
  o  ac36a1f9437e 'A'
