  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False

setup backing repo

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS

test subtree merge path validation
  $ hg go -q $B
  $ hg subtree cp -r $A --from-path foo --to-path bar -m "subtree copy foo -> bar"
  copying foo to bar
  $ hg subtree merge --from-path foo --to-path not-exists
  abort: path 'not-exists' does not exist in commit d7a063467d35
  [255]
  $ hg subtree merge --from-path not-exists --to-path bar
  abort: path 'not-exists' does not exist in commit d7a063467d35
  [255]
  $ hg subtree merge --from-path foo/bar --to-path foo
  abort: overlapping --from-path 'foo/bar' and --to-path 'foo'
  [255]
  $ hg subtree merge --from-path foo --to-path foo/bar
  abort: overlapping --from-path 'foo' and --to-path 'foo/bar'
  [255]
