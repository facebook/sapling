  $ setconfig diff.git=True
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1
  $ enable morestatus
  $ setconfig morestatus.show=True

An explicit no-commit copy creates pending state without invoking the editor:

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |   # B/bar/x = ccc\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ sl go $B -q
  $ sl subtree copy -r $A --from-path foo --to-path foo2 --no-commit
  copying foo to foo2
  (subtree copy changes awaiting commit)
  $ test "$(sl log -r . -T '{node}')" = "$B"
  $ ls
  bar
  foo
  foo2
  $ test -e .sl/subtree-copy-state
  $ cat .sl/subtree-copy-state
  [{"deepcopies":[{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"foo2"}],"v":1}]
  $ sl subtree copy -r $A --from-path foo --to-path foo3 --no-commit
  abort: subtree copy in progress
  (use 'sl commit' to continue or
       'sl goto . --clean' to abort - WARNING: will destroy uncommitted changes)
  [255]
  $ sl status
  A foo2/x
  
  # The repository is in an unfinished *subtree copy* state.
  # To continue:                sl commit
  # To abort:                   sl goto . --clean (WARNING: will destroy uncommitted changes)
  $ test "$(sl log -r . -T '{node}')" = "$B"
  $ ls
  bar
  foo
  foo2
  $ sl commit -m explicit
  $ test ! -e .sl/subtree-copy-state
  $ sl subtree inspect -r .
  {
    "copies": [
      {
        "version": 1,
        "from_commit": "d908813f0f7c9078810e26aad1e37bdb32013d4b",
        "from_path": "foo",
        "to_path": "foo2",
        "type": "deepcopy"
      }
    ]
  }
  $ sl show
  commit:      d36c2c32c8a2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo2/x
  description:
  explicit
  
  Subtree copy from d908813f0f7c9078810e26aad1e37bdb32013d4b
  - Copied path foo to foo2
  
  
  diff --git a/foo2/x b/foo2/x
  new file mode 100644
  --- /dev/null
  +++ b/foo2/x
  @@ -0,0 +1,1 @@
  +aaa

Subtree merge cannot start during an unfinished subtree copy:

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |   # B/bar/x = ccc\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ sl go $B -q
  $ sl subtree copy -r $A --from-path foo --to-path foo2 --no-commit
  copying foo to foo2
  (subtree copy changes awaiting commit)
  $ sl subtree merge -r $A --from-path foo --to-path bar
  abort: subtree copy in progress
  (use 'sl commit' to continue or
       'sl goto . --clean' to abort - WARNING: will destroy uncommitted changes)
  [255]
  $ sl st
  A foo2/x
  
  # The repository is in an unfinished *subtree copy* state.
  # To continue:                sl commit
  # To abort:                   sl goto . --clean (WARNING: will destroy uncommitted changes)

An aborted commit message leaves completed copy metadata for a later commit:

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ sl go $B -q

  $ HGEDITOR='echo >' sl subtree copy -r $A --from-path foo --to-path bar
  copying foo to bar
  abort: empty commit message
  [255]
  $ cat .sl/subtree-copy-state
  [{"deepcopies":[{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"bar"}],"v":1}]

Only a full, new commit can consume pending subtree metadata:

  $ sl commit bar/x -m partial
  abort: cannot partially commit pending subtree copy changes
  (run 'sl commit' to commit the subtree copy changes)
  [255]

  $ sl commit -m recovered
  $ test ! -e .sl/subtree-copy-state
  $ sl log -r . -T '{desc}\n'
  recovered
  
  Subtree copy from d908813f0f7c9078810e26aad1e37bdb32013d4b
  - Copied path foo to bar
  $ sl subtree inspect -r .
  {
    "copies": [
      {
        "version": 1,
        "from_commit": "d908813f0f7c9078810e26aad1e37bdb32013d4b",
        "from_path": "foo",
        "to_path": "bar",
        "type": "deepcopy"
      }
    ]
  }

A no-op copy does not leave metadata for a later commit:

  $ newclientrepo
  $ drawdag <<'EOS'
  > A   # A/foo/x = same\n
  >     # A/bar/x = same\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ sl go $A -q
  $ sl subtree copy --from-path foo --to-path bar --force -m no-op
  removing bar/x
  copying foo to bar
  $ test ! -e .sl/subtree-copy-state

  $ sl subtree copy --from-path foo --to-path bar --force --no-commit
  removing bar/x
  copying foo to bar
  (subtree copy changes awaiting commit)
  $ test -e .sl/subtree-copy-state
  $ sl commit -m no-op
  nothing changed
  [1]
  $ test ! -e .sl/subtree-copy-state

Malformed state is rejected before commit processing:

  $ printf '[{"deepcopies":[{"extra":true,"from_commit":"0000000000000000000000000000000000000000","from_path":"foo","to_path":"bar"}],"v":1}]\n' > .sl/subtree-copy-state
  $ sl commit -m invalid
  abort: invalid subtree copy state: expected deep copy metadata
  (use 'sl goto . --clean' to discard the pending changes)
  [255]
  $ rm .sl/subtree-copy-state

State from one copy operation must use one source commit:

  $ printf '[{"deepcopies":[{"from_commit":"0000000000000000000000000000000000000000","from_path":"foo","to_path":"bar"},{"from_commit":"1111111111111111111111111111111111111111","from_path":"foo","to_path":"baz"}],"v":1}]\n' > .sl/subtree-copy-state
  $ sl commit -m invalid
  abort: invalid subtree copy state: expected one source commit
  (use 'sl goto . --clean' to discard the pending changes)
  [255]
  $ rm .sl/subtree-copy-state

Clean goto restores tracked files and clears the pending copy state:

  $ newclientrepo
  $ setconfig checkout.use-rust=True
  $ drawdag <<'EOS'
  > A   # A/foo/x = aaa\n
  >     # A/foo/y = bbb\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ sl go $A -q
  $ sl subtree copy -r $A --from-path foo --to-path bar --no-commit
  copying foo to bar
  (subtree copy changes awaiting commit)
  $ test -e .sl/subtree-copy-state
  $ sl goto . --clean -q
  $ test ! -e .sl/subtree-copy-state
  $ sl status
  ? bar/x
  ? bar/y
  $ sl clean && rm -r bar

Python clean goto also clears the pending copy state:

  $ sl subtree copy -r $A --from-path foo --to-path bar --no-commit
  copying foo to bar
  (subtree copy changes awaiting commit)
  $ test -e .sl/subtree-copy-state
  $ sl goto . --clean -q --config checkout.use-rust=False
  $ test ! -e .sl/subtree-copy-state
