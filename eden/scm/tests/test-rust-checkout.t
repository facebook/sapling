#debugruntest-compatible

  $ configure modernclient
  $ setconfig checkout.use-rust=true

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
  [255]
