#debugruntest-compatible

  $ configure modernclient
  $ setconfig checkout.use-rust=true
  $ setconfig experimental.nativecheckout=true

  $ newclientrepo
  $ drawdag <<'EOS'
  > A   # A/foo = foo
  >     # A/bar = bar
  > EOS

Unknown file w/ different content - conflict:
  $ echo nope > foo
  $ hg go $A
  foo: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]


Respect merge marker file:
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo = changed
  > |
  > A   # A/foo = foo
  > EOS

  $ hg go -qC $A
  $ echo diverged > foo
  $ hg go -q --merge $B
  warning: 1 conflicts while merging foo! (edit, then use 'hg resolve --mark')
  [1]

  $ hg go $B
  abort: goto --merge in progress
  (use 'hg goto --continue' to continue or
       'hg goto --clean' to abort - WARNING: will destroy uncommitted changes)
  [255]

Run it again to make sure we didn't clear out state file:
  $ hg go $B
  abort: goto --merge in progress
  (use 'hg goto --continue' to continue or
       'hg goto --clean' to abort - WARNING: will destroy uncommitted changes)
  [255]

  $ hg go --continue
  abort: not in an interrupted goto state
  [255]

  $ hg resolve --mark foo
  (no more unresolved files)
  continue: hg goto --continue

  $ hg go --continue
  $ hg st
  M foo
  ? foo.orig


Can continue interrupted checkout:
  $ newclientrepo
  $ drawdag <<'EOS'
  > A   # A/foo = foo
  >     # A/bar = bar
  > EOS

  $ hg go -q null
  $ FAILPOINTS=checkout-post-progress=return hg go $A
  abort: checkout error: Error set by checkout-post-progress FAILPOINTS
  [255]

  $ hg whereami
  0000000000000000000000000000000000000000

  $ hg go --continue $A --rev $A
  abort: can't specify a destination commit and --continue
  [255]

  $ LOG=checkout=debug hg go -q --continue 2>&1 | grep skipped_count
  DEBUG apply_store: checkout: skipped files based on progress skipped_count=3
  $ hg st
  $ tglog
  @  a19fc4bcafed 'A'

  $ hg go --continue
  abort: not in an interrupted goto state
  [255]


Don't fail with open files that can't be deleted:
  $ newclientrepo unlink_fail
  $ drawdag <<'EOS'
  > B   # B/foo = (removed)
  > |   # B/bar = different
  > |
  > A   # A/foo = foo
  >     # A/bar = bar
  > EOS


  $ hg go -q $A

    with open("unlink_fail/foo"), open("unlink_fail/bar"):

      $ hg go $B
      update failed to remove foo: Can't remove file "*foo": The process cannot access the file because it is being used by another process. (os error 32)! (glob) (windows !)
      2 files updated, 0 files merged, 1 files removed, 0 files unresolved


Respect other repo states:
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo = two
  > 
  > A   # A/foo = one
  > EOS

  $ hg go -q $A
  $ hg graft -r $B
  grafting e57212eac5db "B"
  merging foo
  warning: 1 conflicts while merging foo! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  
  $ hg go $B
  abort: graft in progress
  (use 'hg graft --continue' to continue or
       'hg graft --abort' to abort)
  [255]

Various invalid arg combos:

  $ newclientrepo
  $ hg go foo --rev bar
  abort: goto requires exactly one destination commit but got: ["foo", "bar"]
  [255]

  $ hg go
  abort: you must specify a destination to update to, for example "hg goto main".
  [255]
