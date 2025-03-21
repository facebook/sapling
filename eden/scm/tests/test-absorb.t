
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig 'experimental.evolution='
  $ enable absorb

  $ cat >> $TESTTMP/dummyamend.py << 'EOF'
  > from sapling import commands, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('amend', [], '')
  > def amend(ui, repo, *pats, **opts):
  >     return 3
  > EOF
  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > amend=$TESTTMP/dummyamend.py
  > [absorb]
  > amendflag = correlated
  > EOF

  $ newclientrepo

# Do not crash with empty repo:

  $ hg absorb
  abort: no changeset to change
  [255]

# Make some commits:

  $ for i in `seq 5`; do
  >     echo $i >> a
  >     hg commit -A a -q -m "commit $i"
  > done

# Change a few lines:

  $ cat > a << 'EOF'
  > 1a
  > 2b
  > 3
  > 4d
  > 5e
  > EOF

# Preview absorb changes:

  $ hg absorb --dry-run
  showing changes for a
          @@ -0,2 +0,2 @@
  4ec16f8 -1
  5c5f952 -2
  4ec16f8 +1a
  5c5f952 +2b
          @@ -3,2 +3,2 @@
  ad8b8b7 -4
  4f55fa6 -5
  ad8b8b7 +4d
  4f55fa6 +5e
  
  4 changesets affected
  4f55fa6 commit 5
  ad8b8b7 commit 4
  5c5f952 commit 2
  4ec16f8 commit 1

# Run absorb:

  $ hg absorb --apply-changes
  showing changes for a
          @@ -0,2 +0,2 @@
  4ec16f8 -1
  5c5f952 -2
  4ec16f8 +1a
  5c5f952 +2b
          @@ -3,2 +3,2 @@
  ad8b8b7 -4
  4f55fa6 -5
  ad8b8b7 +4d
  4f55fa6 +5e
  
  4 changesets affected
  4f55fa6 commit 5
  ad8b8b7 commit 4
  5c5f952 commit 2
  4ec16f8 commit 1
  2 of 2 chunks applied
  $ hg annotate a
  241ace8326d0: 1a
  9b19176bb127: 2b
  484c6ac0cea3: 3
  04c8ba6df782: 4d
  2f7ba78d6abc: 5e

# Delete a few lines and related commits will be removed if they will be empty:

  $ cat > a << 'EOF'
  > 2b
  > 4d
  > EOF
  $ hg absorb -a
  showing changes for a
          @@ -0,1 +0,0 @@
  241ace8 -1a
          @@ -2,1 +1,0 @@
  484c6ac -3
          @@ -4,1 +2,0 @@
  2f7ba78 -5e
  
  3 changesets affected
  2f7ba78 commit 5
  484c6ac commit 3
  241ace8 commit 1
  3 of 3 chunks applied
  $ hg annotate a
  17567d7d67ff: 2b
  c04dc600ace7: 4d
  $ hg log -T '{rev} {desc}\n' -Gp
  @  12 commit 4
  │  diff -r 17567d7d67ff -r c04dc600ace7 a
  │  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  │  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  │  @@ -1,1 +1,2 @@
  │   2b
  │  +4d
  │
  o  11 commit 2
  │  diff -r 16674334a991 -r 17567d7d67ff a
  │  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  │  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  │  @@ -0,0 +1,1 @@
  │  +2b
  │
  o  10 commit 1

# Non 1:1 map changes will be ignored:

  $ echo 1 > a
  $ hg absorb
  showing changes for a
          @@ -0,2 +0,1 @@
          -2b
          -4d
          +1
  nothing to absorb
  [1]

# Insertaions:

  $ cat > a << 'EOF'
  > insert before 2b
  > 2b
  > 4d
  > insert aftert 4d
  > EOF
  $ hg absorb -aq
  $ hg status
  $ hg annotate a
  b493c37385e0: insert before 2b
  b493c37385e0: 2b
  ec7f94714b2a: 4d
  ec7f94714b2a: insert aftert 4d

# Bookmarks are moved:

  $ hg bookmark -r '.^' b1
  $ hg bookmark -r '.' b2
  $ hg bookmark ba
  $ hg bookmarks
     b1                        b493c37385e0
     b2                        ec7f94714b2a
   * ba                        ec7f94714b2a
  $ sed -i 's/insert/INSERT/' a
  $ hg absorb -aq
  $ hg status
  $ hg bookmarks
     b1                        701492731deb
     b2                        a792c8f2d460
   * ba                        a792c8f2d460

# Non-mofified files are ignored:

  $ touch b
  $ hg commit -A b -m b
  $ touch c
  $ hg add c
  $ hg rm b
  $ hg absorb
  nothing to absorb
  [1]
  $ sed -i 's/INSERT/Insert/' a
  $ hg absorb -a
  showing changes for a
          @@ -0,1 +0,1 @@
  7014927 -INSERT before 2b
  7014927 +Insert before 2b
          @@ -3,1 +3,1 @@
  a792c8f -INSERT aftert 4d
  a792c8f +Insert aftert 4d
  
  2 changesets affected
  a792c8f commit 4
  7014927 commit 2
  2 of 2 chunks applied
  $ hg status
  A c
  R b

# Public commits will not be changed:

  $ hg debugmakepublic '.^^'
  $ sed -i 's/Insert/insert/' a
  $ hg absorb -n
  showing changes for a
          @@ -0,1 +0,1 @@
          -Insert before 2b
          +insert before 2b
          @@ -3,1 +3,1 @@
  93862a2 -Insert aftert 4d
  93862a2 +insert aftert 4d
  
  1 changeset affected
  93862a2 commit 4
  $ hg absorb -a
  showing changes for a
          @@ -0,1 +0,1 @@
          -Insert before 2b
          +insert before 2b
          @@ -3,1 +3,1 @@
  93862a2 -Insert aftert 4d
  93862a2 +insert aftert 4d
  
  1 changeset affected
  93862a2 commit 4
  1 of 2 chunks applied
  $ hg diff -U 0
  diff -r f4cdbecd99bf a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -Insert before 2b
  +insert before 2b
  $ hg annotate a
  e57f8b115d87: Insert before 2b
  e57f8b115d87: 2b
  fa39e2e50dfe: 4d
  fa39e2e50dfe: insert aftert 4d

# Make working copy clean:

  $ hg revert -q -C a b
  $ hg forget c
  $ rm c
  $ hg status

# Merge commit will not be changed:

  $ echo 1 > m1
  $ hg commit -A m1 -m m1
  $ hg bookmark -q -i m1
  $ hg goto -q '.^'
  $ echo 2 > m2
  $ hg commit -q -A m2 -m m2
  $ hg merge -q m1
  $ hg commit -m merge
  $ hg bookmark -d m1
  $ hg log -G -T '{rev} {desc} {phase}\n'
  @    25 merge draft
  ├─╮
  │ o  24 m2 draft
  │ │
  o │  23 m1 draft
  ├─╯
  o  22 b draft
  │
  o  21 commit 4 draft
  │
  o  18 commit 2 public
  │
  o  10 commit 1 public
  $ echo 2 >> m1
  $ echo 2 >> m2
  $ hg absorb -a
  abort: no changeset to change
  [255]
  $ hg revert -q -C m1 m2

# Use a new repo:

  $ newrepo

# Make some commits to multiple files:

  $ for f in a b; do
  >     for i in 1 2; do
  >         echo "$f line $i" >> $f
  >         hg commit -A $f -m "commit $f $i" -q
  >     done
  > done

# Use pattern to select files to be fixed up:

  $ for i in a b; do sed -i 's/line/Line/' $i; done
  $ hg status
  M a
  M b
  $ hg absorb -a a
  showing changes for a
          @@ -0,2 +0,2 @@
  6905bbb -a line 1
  4472dd5 -a line 2
  6905bbb +a Line 1
  4472dd5 +a Line 2
  
  2 changesets affected
  4472dd5 commit a 2
  6905bbb commit a 1
  1 of 1 chunk applied
  $ hg status
  M b
  $ hg absorb -a --exclude b
  nothing to absorb
  [1]
  $ hg absorb -a b
  showing changes for b
          @@ -0,2 +0,2 @@
  2156a2c -b line 1
  3440b6f -b line 2
  2156a2c +b Line 1
  3440b6f +b Line 2
  
  2 changesets affected
  3440b6f commit b 2
  2156a2c commit b 1
  1 of 1 chunk applied
  $ hg status
  $ cat a b
  a Line 1
  a Line 2
  b Line 1
  b Line 2

# Test config option absorb.maxstacksize:

  $ for i in a b; do sed -i 's/Line/line/' $i; done
  $ hg log -T '{rev}:{node} {desc}\n'
  9:7187128ea253f1046db1cb899a8fb84e5adf55f0 commit b 2
  8:5998ceb9fa0dd3916e5b3bd95f21dfe901357230 commit b 1
  5:b1717940ba15276e6ca31250ac4603f9effe8d0e commit a 2
  4:cb19766bbe10477222cba73b7c9351e4c38bce0b commit a 1
  $ hg --config 'absorb.maxstacksize=1' absorb -n
  absorb: only the recent 1 changesets will be analysed
  showing changes for a
          @@ -0,2 +0,2 @@
          -a Line 1
          -a Line 2
          +a line 1
          +a line 2
  showing changes for b
          @@ -0,2 +0,2 @@
          -b Line 1
  7187128 -b Line 2
          +b line 1
  7187128 +b line 2
  
  1 changeset affected
  7187128 commit b 2

# Test obsolete markers creation:

  $ cat >> $HGRCPATH << 'EOF'
  > [experimental]
  > evolution=createmarkers
  > EOF

  $ hg --config 'absorb.maxstacksize=3' sf -a
  absorb: only the recent 3 changesets will be analysed
  showing changes for a
          @@ -0,2 +0,2 @@
          -a Line 1
  b171794 -a Line 2
          +a line 1
  b171794 +a line 2
  showing changes for b
          @@ -0,2 +0,2 @@
  5998ceb -b Line 1
  7187128 -b Line 2
  5998ceb +b line 1
  7187128 +b line 2
  
  3 changesets affected
  7187128 commit b 2
  5998ceb commit b 1
  b171794 commit a 2
  2 of 2 chunks applied
  $ hg log -T '{rev}:{node|short} {desc} {get(extras, "absorb_source")}\n'
  12:5c0c8e4475ee commit b 2 7187128ea253f1046db1cb899a8fb84e5adf55f0
  11:adb6517fe29c commit b 1 5998ceb9fa0dd3916e5b3bd95f21dfe901357230
  10:1e3ad81440c1 commit a 2 b1717940ba15276e6ca31250ac4603f9effe8d0e
  4:cb19766bbe10 commit a 1 6905bbb02e4e4ae007d5e6738558e0bbbcb08878
  $ hg absorb -a
  showing changes for a
          @@ -0,1 +0,1 @@
  cb19766 -a Line 1
  cb19766 +a line 1
  
  1 changeset affected
  cb19766 commit a 1
  1 of 1 chunk applied
  $ hg log -T '{rev}:{node|short} {desc} {get(extras, "absorb_source")}\n'
  16:c972e4e4c278 commit b 2 5c0c8e4475ee09f0c4e0123f4bf3360199dae620
  15:b68a3d5de718 commit b 1 adb6517fe29c18543f21ff85fe839aca6ab4b89e
  14:04e06cb87728 commit a 2 1e3ad81440c1845beb9779cf7cfc3890f4db1add
  13:b3c856cfc07f commit a 1 cb19766bbe10477222cba73b7c9351e4c38bce0b

# Test config option absorb.amendflags and running as a sub command of amend:

  $ hg amend -h
  hg amend
  
  (no help text available)
  
  Options:
  
    --correlated incorporate corrections into stack. see 'hg help absorb' for
                 details
  
  (some details hidden, use --verbose to show complete help)

# Test binary file

  >>> with open("c", "wb") as f: f.write(bytearray([0, 1, 2, 10])) and None

  $ hg commit -A c -m 'c is a binary file'
  $ echo c >> c

  $ cat b
  b line 1
  b line 2

  $ cat > b << 'EOF'
  > b line 1
  > INS
  > b line 2
  > EOF

  $ echo END >> b
  $ hg rm a
  $ echo y | hg amend --correlated --config 'ui.interactive=1'
  showing changes for b
          @@ -1,0 +1,1 @@
          +INS
          @@ -2,0 +3,1 @@
  c972e4e +END
  
  1 changeset affected
  c972e4e commit b 2
  apply changes (yn)?  y
  1 of 2 chunks applied
  
  # changes not applied and left in working copy:
  # M b : 1 modified chunks were ignored
  # M c : unsupported file type (ex. binary or link)
  # R a : removed files were ignored

# Executable files:

  $ cat >> $HGRCPATH << 'EOF'
  > [diff]
  > git=True
  > EOF

#if execbit
  $ newrepo
  $ echo > foo.py
  $ chmod +x foo.py
  $ hg add foo.py
  $ hg commit -mfoo

  $ echo bla > foo.py
  $ hg absorb --dry-run
  showing changes for foo.py
          @@ -0,1 +0,1 @@
  99b4ae7 -
  99b4ae7 +bla
  
  1 changeset affected
  99b4ae7 foo
  $ hg absorb --apply-changes
  showing changes for foo.py
          @@ -0,1 +0,1 @@
  99b4ae7 -
  99b4ae7 +bla
  
  1 changeset affected
  99b4ae7 foo
  1 of 1 chunk applied
  $ hg diff -c .
  diff --git a/foo.py b/foo.py
  new file mode 100755
  --- /dev/null
  +++ b/foo.py
  @@ -0,0 +1,1 @@
  +bla
  $ hg diff
#endif

# Remove lines may delete changesets:

  $ newrepo
  $ cat > a << 'EOF'
  > 1
  > 2
  > EOF
  $ hg commit -m a12 -A a
  $ cat > b << 'EOF'
  > 1
  > 2
  > EOF
  $ hg commit -m b12 -A b
  $ echo 3 >> b
  $ hg commit -m b3
  $ echo 4 >> b
  $ hg commit -m b4
  $ echo 1 > b
  $ echo 3 >> a
  $ hg absorb -n
  showing changes for a
          @@ -2,0 +2,1 @@
  bfafb49 +3
  showing changes for b
          @@ -1,3 +1,0 @@
  1154859 -2
  30970db -3
  a393a58 -4
  
  4 changesets affected
  a393a58 b4
  30970db b3
  1154859 b12
  bfafb49 a12
  $ hg absorb -av | grep became
  bfafb49242db: 1 file(s) changed, became 1a2de97fc652
  115485984805: 2 file(s) changed, became 0c930dfab74c
  30970dbf7b40: became empty and was dropped
  a393a58b9a85: became empty and was dropped
  $ hg log -T '{rev} {desc}\n' -Gp
  @  5 b12
  │  diff --git a/b b/b
  │  new file mode 100644
  │  --- /dev/null
  │  +++ b/b
  │  @@ -0,0 +1,1 @@
  │  +1
  │
  o  4 a12
     diff --git a/a b/a
     new file mode 100644
     --- /dev/null
     +++ b/a
     @@ -0,0 +1,3 @@
     +1
     +2
     +3

# Only with commit deletion:

  $ newrepo
  $ touch a
  $ hg ci -m 'empty a' -A a
  $ echo 1 >> a
  $ hg ci -m 'append to a'
  $ rm a
  $ touch a
  $ HGPLAIN=1 hg absorb
  showing changes for a
          @@ -0,1 +0,0 @@
  d235271 -1
  
  1 changeset affected
  d235271 append to a
  apply changes (yn)?  y
  1 of 1 chunk applied

# Can specify date

  $ newrepo
  $ echo foo > a
  $ hg ci -m 'a' -A a
  $ echo bar >> a
  $ HGPLAIN=1 hg absorb -qa -d 2022-07-12T00:00:00
  $ hg log -r . -T '{date}\n'
  1657584000.00
