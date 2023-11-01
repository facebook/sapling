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
  abort: outstanding merge conflicts
  (use 'hg resolve --list' to list, 'hg resolve --mark FILE' to mark resolved)
  [255]

Run it again to make sure we didn't clear out state file:
  $ hg go $B
  abort: outstanding merge conflicts
  (use 'hg resolve --list' to list, 'hg resolve --mark FILE' to mark resolved)
  [255]

  $ hg go --continue
  abort: outstanding merge conflicts
  (use 'hg resolve --list' to list, 'hg resolve --mark FILE' to mark resolved)
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
  abort: oh no!
  [255]

  $ hg whereami
  0000000000000000000000000000000000000000

  $ hg go --continue $A --rev $A
  abort: checkout requires exactly one destination commit but got: ["a19fc4bcafede967b22a29cd9af839765fff19b7", "a19fc4bcafede967b22a29cd9af839765fff19b7", "a19fc4bcafede967b22a29cd9af839765fff19b7"]
  [255]

  $ LOG=checkout=debug hg go -q --continue 2>&1 | grep skipped_count
  DEBUG apply_store: checkout: skipped files based on progress skipped_count=3
  $ hg st
  $ tglog
  @  a19fc4bcafed 'A'

  $ hg go --continue
  abort: not in an interrupted update state
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

