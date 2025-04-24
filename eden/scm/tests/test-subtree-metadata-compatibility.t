  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1


create an extension to enable non-test subtree extra
  $ cat > $TESTTMP/subtree.py <<EOF
  > from sapling.utils import subtreeutil
  > def extsetup(ui):
  >     subtreeutil.SUBTREE_KEY = "subtree"
  > EOF

test subtree inspect for subtree metadata
  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # C/foo/x = aaa\nccc\n
  > |
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS

  $ hg log -G -T '{node|short} {desc}\n'
  o  eed7ada653c1 C
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
  $ hg go -q $C
  $ hg subtree copy -r $B --from-path foo --to-path foo2
  copying foo to foo2
  $ hg subtree inspect
  {
    "copies": [
      {
        "version": 1,
        "from_commit": "9998a5c40732fc326e6f10a4f14437c7f8e8e7ae",
        "from_path": "foo",
        "to_path": "foo2",
        "type": "deepcopy"
      }
    ]
  }
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  test_subtree=[{"deepcopies":[{"from_commit":"9998a5c40732fc326e6f10a4f14437c7f8e8e7ae","from_path":"foo","to_path":"foo2"}],"v":1}]

enable the new subtree key
  $ setconfig extensions.subtreetestoverride=$TESTTMP/subtree.py

tofix: make sure inspect command works for existing metadata
  $ hg subtree inspect -r .
  no subtree metadata found for commit ceef88fb118b

tofix: merge should use commit B (9998a5c40732) as the merge base
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg subtree merge --from-path foo --to-path foo2 -t :merge3
  computing merge base (timeout: 120 seconds)...
  merge base: eed7ada653c1
  merging foo2/x and foo/x to foo2/x
  warning: 1 conflicts while merging foo2/x! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg st
  M foo2/x
  ? foo2/x.orig
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,1 +1,8 @@
   aaa
  +<<<<<<< working copy: da6195d0c136 - test: update foo2
  +||||||| base
  +ccc
  +=======
  +ccc
  +source
  +>>>>>>> merge rev:    da6195d0c136 - test: update foo2
