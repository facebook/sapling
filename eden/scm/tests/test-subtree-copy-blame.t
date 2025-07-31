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

test blame on bar/x
  $ hg blame bar/x
  d908813f0f7c: aaa
  e8c35cfd53d9: bbb

update foo/x and then run blame
  $ echo "ccc" >> bar/x
  $ hg ci -m "update bar/x"
  $ hg log -r . -T '{node|short}\n' 
  999d230b9730
  $ hg blame bar/x
  d908813f0f7c: aaa
  e8c35cfd53d9: bbb
  999d230b9730: ccc

all lines are modified in the working copy
  $ cat > bar/x << EOF
  > 111
  > 222
  > 333
  > EOF
  $ hg blame -r "wdir()" bar/x
  999d230b9730+: 111
  999d230b9730+: 222
  999d230b9730+: 333
