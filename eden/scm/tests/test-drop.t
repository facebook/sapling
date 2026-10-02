
  $ eagerepo
  $ enable rebase drop
  $ enable morestatus
  $ setconfig morestatus.show=true

Drop is listed with the stack commands in the help home page:
  $ sl help | grep '^ drop '
   drop          remove changesets from the middle of a stack

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

Conflicts while rebasing descendants interrupt the drop:
  $ newclientrepo
  $ drawdag <<'EOS'
  > E  # E/f = 5\n
  > |
  > D  # D/f = 4\n
  > |
  > C  # C/f = 3\n
  > |
  > B  # B/f = 2\n
  > |
  > A  # A/f = 1\n
  > EOS
  $ sl drop -r $B -r $D
  dropping changeset d24cfa: B
  dropping changeset 8d45fb: D
  rebasing 11b100dbbeb1 "C"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl drop --continue)
  [1]

The interrupted drop is reported by status, blocks other commands, and is
what ISL is told to continue or abort:
  $ sl status
  M C
  M f
  ? f.orig
  
  # The repository is in an unfinished *drop* state.
  # Unresolved merge conflicts (1):
  # 
  #     f
  # 
  # To mark files as resolved:  sl resolve --mark FILE
  # To continue:                sl drop --continue
  # To abort:                   sl drop --abort
  $ sl goto -q $A
  abort: drop in progress
  (use 'sl drop --continue' to continue or
       'sl drop --abort' to abort)
  [255]
  $ sl resolve --tool internal:dumpjson --all | pp | head -8
  [
    {
      "command": "drop",
      "command_details": {
        "cmd": "drop",
        "to_abort": "drop --abort",
        "to_continue": "drop --continue"
      },

Resolving the conflict and continuing resumes the drop, stopping again at the
next conflict, and `sl continue` continues the drop too:
  $ sl resolve --tool internal:other --all
  (no more unresolved files)
  continue: sl drop --continue
  $ sl drop --continue
  rebasing 11b100dbbeb1 "C"
  rebasing cb77bf3d5069 "E"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl drop --continue)
  [1]
  $ sl resolve --tool internal:other --all
  (no more unresolved files)
  continue: sl drop --continue
  $ sl continue
  already rebased 11b100dbbeb1 "C" as b7f0d49ae613
  rebasing cb77bf3d5069 "E"
  $ tglog
  o  8b9def33bb63 'E'
  │
  o  b7f0d49ae613 'C'
  │
  o  ac36a1f9437e 'A'
  $ sl status
  ? f.orig

There is nothing left to continue or abort:
  $ sl drop --continue
  abort: no drop in progress
  [255]
  $ sl drop --abort
  abort: no drop in progress
  [255]

Aborting an interrupted drop restores the stack and the working copy:
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
  $ sl goto -q $D
  $ sl drop -r $B -r $D
  dropping changeset d24cfa: B
  dropping changeset 7abc7f: D
  rebasing 11b100dbbeb1 "C"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl drop --continue)
  [1]
  $ sl drop --continue -r $B
  abort: cannot specify revisions with --continue or --abort
  [255]
  $ sl drop --abort
  rebase aborted
  drop aborted
  $ tglog
  @  7abc7f013e72 'D'
  │
  o  11b100dbbeb1 'C'
  │
  o  d24cfa11bac1 'B'
  │
  o  ac36a1f9437e 'A'
  $ sl status
  ? f.orig

Continuing the rebase directly still leaves the drop to finish:
  $ sl drop -r $B -r $D
  dropping changeset d24cfa: B
  dropping changeset 7abc7f: D
  rebasing 11b100dbbeb1 "C"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl drop --continue)
  [1]
  $ sl resolve --tool internal:other --all
  (no more unresolved files)
  continue: sl drop --continue
  $ sl rebase --continue
  rebasing 11b100dbbeb1 "C"
  $ sl status
  ? f.orig
  
  # The repository is in an unfinished *drop* state.
  # To continue:                sl drop --continue
  # To abort:                   sl drop --abort
  $ sl drop --continue
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  working directory now at b7f0d49ae613
  $ tglog
  @  b7f0d49ae613 'C'
  │
  o  ac36a1f9437e 'A'

Dropping from two stacks rebases both in a single rebase, so aborting after a
conflict in the second stack also undoes the clean rebase of the first. This
holds with and without in-memory rebase (which production uses):
  $ twostacks() {
  >   newclientrepo
  >   drawdag <<'EOS'
  > X3 Y3  # Y3/g = 3\n
  > |  |
  > X2 Y2  # Y2/g = 2\n
  > |  |
  > X1 Y1  # Y1/g = 1\n
  >  \ |
  >    A
  > EOS
  >   sl goto -q $Y3
  > }
  $ twostacks
  $ sl drop -r $X2 -r $Y2
  dropping changeset 3e920a: X2
  dropping changeset 9a761e: Y2
  rebasing 0e071e9f07f3 "X3"
  rebasing f5d6da64e2a3 "Y3"
  merging g
  warning: 1 conflicts while merging g! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl drop --continue)
  [1]
  $ sl log -G -T '{desc}\n'
  o  X3
  │
  │ @  Y3
  │ │
  │ o  Y2
  │ │
  │ │ x  X3
  │ │ │
  │ @ │  Y1
  │ │ │
  │ │ o  X2
  ├───╯
  o │  X1
    │
    o  A
  $ sl drop --abort
  rebase aborted
  drop aborted
  $ sl log -G -T '{desc}\n'
  @  Y3
  │
  o  Y2
  │
  │ o  X3
  │ │
  o │  Y1
  │ │
  │ o  X2
  │ │
  │ o  X1
  │
  o  A

  $ twostacks
  $ sl drop -r $X2 -r $Y2 --config rebase.experimental.inmemory=true
  dropping changeset 3e920a: X2
  dropping changeset 9a761e: Y2
  rebasing 0e071e9f07f3 "X3"
  rebasing f5d6da64e2a3 "Y3"
  merging g
  hit merge conflicts (in g); switching to on-disk merge
  rebasing f5d6da64e2a3 "Y3"
  merging g
  warning: 1 conflicts while merging g! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl drop --continue)
  [1]
  $ sl drop --abort
  rebase aborted
  drop aborted
  $ sl log -G -T '{desc}\n'
  @  Y3
  │
  o  Y2
  │
  │ o  X3
  │ │
  o │  Y1
  │ │
  │ o  X2
  │ │
  │ o  X1
  │
  o  A

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
