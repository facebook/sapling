  $ setconfig diff.git=True
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

test subtree copy
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = aaa\nbbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $B -q
  $ hg subtree cp -r $B --from-path foo --to-path bar -m "subtree copy foo -> bar"
  copying foo to bar
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  80d62d83076f subtree copy foo -> bar
  │
  o  e8c35cfd53d9 B
  │
  o  d908813f0f7c A

tofix: test blame on bar/x
  $ hg blame bar/x
  80d62d83076f: aaa
  80d62d83076f: bbb

tofix: update foo/x and then run blame
  $ echo "ccc" >> bar/x
  $ hg ci -m "update bar/x"
  $ hg log -r . -T '{node|short}\n' 
  999d230b9730
  $ hg blame bar/x
  80d62d83076f: aaa
  80d62d83076f: bbb
  999d230b9730: ccc
