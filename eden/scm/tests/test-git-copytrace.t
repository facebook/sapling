#require git no-windows no-eden

  $ eagerepo
  $ . $TESTDIR/git.sh
  $ setconfig diff.git=True
  $ enable rebase

  $ setupconfig() {
  >   setconfig copytrace.dagcopytrace=True
  > }

Prepare repo

  $ hg init --git repo1
  $ cd repo1
  $ cat > a << EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ hg ci -q -Am 'add a'

Test copytrace

  $ hg rm a
  $ cat > b << EOF
  > 1
  > 2
  > 3
  > 4
  > EOF
  $ hg ci -q -Am 'mv a -> b'
  $ hg log -T '{node|short}\n' -r .
  fb4ff23de3ea

Default similarity threshold 0.8 should work

  $ hg debugcopytrace -s .~1 -d . a
  {"a": "b"}

High similarity threshold should fail to find the rename
  $ hg debugcopytrace -s .~1 -d . a --config copytrace.similarity-threshold=0.91
  {"a": "the missing file was deleted by commit fb4ff23de3ea in the branch rebasing onto"}

Low max rename edit cost should fail to find the rename
  $ hg debugcopytrace -s .~1 -d . a --config copytrace.max-edit-cost=0
  {"a": "the missing file was deleted by commit fb4ff23de3ea in the branch rebasing onto"}

Test missing files in source side

  $ hg init --git repo2
  $ cd repo2
  $ setupconfig
  $ drawdag <<'EOS'
  > C   # C/y = 1\n (renamed from x)
  > |   # C/C = (removed)
  > |
  > | B # B/x = 1\n2\n
  > | | # B/B = (removed)
  > |/
  > A   # A/x = 1\n
  >     # A/A = (removed)
  > EOS

  $ hg rebase -r $C -d $B
  rebasing 470d2f079ab1 "C"
  merging x and y to y

Test missing files in destination side

  $ hg init --git repo2
  $ cd repo2
  $ setupconfig
  $ drawdag <<'EOS'
  > C   # C/y = 1\n (renamed from x)
  > |   # C/C = (removed)
  > |
  > | B # B/x = 1\n2\n
  > | | # B/B = (removed)
  > |/
  > A   # A/x = 1\n
  >     # A/A = (removed)
  > EOS

  $ hg rebase -r $B -d $C
  rebasing 74b913efe823 "B"
  merging y and x to y

Test path_copies() configs

  $ hg init --git repo2
  $ cd repo2
  $ setupconfig
  $ drawdag <<'EOS'
  > D   # D/z = 1\n (renamed from y)
  > |   # D/D = (removed)
  > C   # C/y = 1\n (renamed from x)
  > |   # C/C = (removed)
  > | B # B/x = 1\n2\n
  > |/
  > A   # A/x = 1\n
  > EOS

  $ hg st -C --change $D
  A z
    y
  R y
  $ hg diff -r $A -r $D
  diff --git a/x b/z
  rename from x
  rename to z
  $ hg diff -r $A -r $D --config copytrace.pathcopiescommitlimit=0
  diff --git a/x b/x
  deleted file mode 100644
  --- a/x
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -1
  diff --git a/z b/z
  new file mode 100644
  --- /dev/null
  +++ b/z
  @@ -0,0 +1,1 @@
  +1
  $ hg diff -r $A -r $D --config copytrace.maxmissingfiles=0
  diff --git a/x b/x
  deleted file mode 100644
  --- a/x
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -1
  diff --git a/z b/z
  new file mode 100644
  --- /dev/null
  +++ b/z
  @@ -0,0 +1,1 @@
  +1
