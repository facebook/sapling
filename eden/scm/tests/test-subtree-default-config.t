configs unrelated to subtree
  $ setconfig diff.git=True

the allow-any-source-commit config makes it easier to wirte tests
  $ setconfig subtree.allow-any-source-commit=True

test subtree merge with top level directories
  $ newclientrepo
  $ drawdag <<'EOS'
  >   D  # D/foo/bar/y = 111\n
  >   |
  > B C  # B/foo/bar/x = 1a\n2\n3\n
  > |/   # C/foo/bar/x = 1\n2\n3a\n
  > A    # A/foo/bar/x = 1\n2\n3\n
  > EOS

  $ hg go $B -q
  $ hg subtree merge -r $D --from-path foo --to-path foo
  abort: path should be at least 2 levels deep: 'foo'
  [255]
  $ cd foo
  $ hg subtree merge -r $D --from-path . --to-path .
  abort: path should be at least 2 levels deep: 'foo'
  [255]
  $ hg subtree merge -r $D --from-path bar --to-path bar
  merge base: 882d8eb0d4d6
  merging bar/x
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
