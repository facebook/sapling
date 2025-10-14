  $ setconfig diff.git=True
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1
  $ setconfig drawdag.defaultfiles=false

test warning about changes outside the specified from path

  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # C/bar/y = 1\n2\n3c\n
  > |   # C/foo/x = 1\n2\n3c\n
  > | B # B/foo/x = 1b\n2\n3\n
  > |/  # A/bar/y = 1\n2\n3\n
  > A   # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B

  $ hg subtree merge --from-path foo --to-path foo -r $C
  warning: changes outside the specified from_path are ignored!
  (use 'hg diff -r a1383e79789b -r 531d8f7a5755 --stat' to see all changed files)
  merge base: a1383e79789b
  merging foo/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff -r a1383e79789b -r 531d8f7a5755 --stat
   bar/y |  2 +-
   foo/x |  2 +-
   2 files changed, 2 insertions(+), 2 deletions(-)
  $ hg diff
  diff --git a/foo/x b/foo/x
  --- a/foo/x
  +++ b/foo/x
  @@ -1,3 +1,3 @@
   1b
   2
  -3
  +3c
