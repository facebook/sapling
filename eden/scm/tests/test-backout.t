#chg-compatible

  $ configure modern
  $ newrepo basic
  $ drawdag << 'EOS'
  > D
  > |
  > C E   # C/A=(removed)
  > |/    # C/B=B1
  > B
  > |
  > A
  > EOS
  $ hg up -qC $D

should complain

  $ hg backout
  abort: please specify a revision to backout
  [255]
  $ hg backout -r $A $B
  abort: please specify just one revision
  [255]
  $ hg backout $E
  abort: cannot backout change that is not an ancestor
  [255]

basic operation

  $ hg backout -d '1000 +0800' $C --no-edit
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  changeset d2f56590172c backs out changeset 2e4218cf3ee0

backout of backout is as if nothing happened

  $ hg backout -d '2000 +0800' tip --no-edit
  removing A
  reverting B
  adding C
  changeset 6916acf22814 backs out changeset d2f56590172c

check the changes

  $ hg log -Gr 'desc(Back)' -T '{desc}' -p --config diff.git=1
  @  Back out "Back out "C""
  │
  │  Original commit changeset: d2f56590172cdiff --git a/A b/A
  │  deleted file mode 100644
  │  --- a/A
  │  +++ /dev/null
  │  @@ -1,1 +0,0 @@
  │  -A
  │  \ No newline at end of file
  │  diff --git a/B b/B
  │  --- a/B
  │  +++ b/B
  │  @@ -1,1 +1,1 @@
  │  -B
  │  \ No newline at end of file
  │  +B1
  │  \ No newline at end of file
  │  diff --git a/C b/C
  │  new file mode 100644
  │  --- /dev/null
  │  +++ b/C
  │  @@ -0,0 +1,1 @@
  │  +C
  │  \ No newline at end of file
  │
  o  Back out "C"
  │
  ~  Original commit changeset: 2e4218cf3ee0diff --git a/A b/A
     new file mode 100644
     --- /dev/null
     +++ b/A
     @@ -0,0 +1,1 @@
     +A
     \ No newline at end of file
     diff --git a/B b/B
     --- a/B
     +++ b/B
     @@ -1,1 +1,1 @@
     -B1
     \ No newline at end of file
     +B
     \ No newline at end of file
     diff --git a/C b/C
     deleted file mode 100644
     --- a/C
     +++ /dev/null
     @@ -1,1 +0,0 @@
     -C
     \ No newline at end of file
  
test --no-commit

  $ hg up -qC $E
  $ hg backout --no-commit .
  removing E
  changeset 49cb92066bfd backed out, don't forget to commit.
  $ hg diff --config diff.git=1
  diff --git a/E b/E
  deleted file mode 100644
  --- a/E
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -E
  \ No newline at end of file

  $ cd ..

Test backing out a mv keeps the blame history even if copytracing is off
  $ hg init mv-backout
  $ cd mv-backout
  $ setconfig experimental.copytrace=off
  $ echo a > foo
  $ hg commit -Aqm a
  $ echo b >> foo
  $ hg commit -Aqm b
  $ hg mv foo bar
  $ hg commit -Aqm move
  $ hg backout -r . -m backout
  removing bar
  adding foo
  changeset b53c2dfb1fb7 backs out changeset 863da64a0012
  $ hg status --change . -C
  A foo
    bar
  R bar
  $ hg blame -c foo
  3e92d79f743a: a
  998f4c3a2bdf: b

  $ cd ..

Make sure editor sees proper message
  $ hg init backout-msg
  $ cd backout-msg
  $ echo a > foo
  $ hg commit -Aqm a
  $ echo b > bar
  $ hg commit -Aqm b
No output means editor was not invoked
  $ HGEDITOR=cat hg backout .
  removing bar
  changeset 4dc95485382f backs out changeset 20b005551096
Now we see the default commit message
  $ hg up -qC 'desc(b)'
  $ HGEDITOR=cat hg backout . --edit
  adding bar
  Back out "Back out "b""
  
  Original commit changeset: 4dc95485382f
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added bar
  changeset 9b702c98cd4b backs out changeset 4dc95485382f
