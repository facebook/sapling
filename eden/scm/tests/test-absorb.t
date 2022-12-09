#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig 'experimental.evolution='
  $ enable absorb

  $ cat >> $TESTTMP/dummyamend.py << 'EOF'
  > from edenscm import commands, registrar
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

  $ newrepo

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
  5: 1a
  6: 2b
  7: 3
  8: 4d
  9: 5e

# Delete a few lines and related commits will be removed if they will be empty:

  $ cat > a << 'EOF'
  > 2b
  > 4d
  > EOF
  $ hg absorb -a
  showing changes for a
          @@ -0,1 +0,0 @@
  f548282 -1a
          @@ -2,1 +1,0 @@
  ff5d556 -3
          @@ -4,1 +2,0 @@
  84e5416 -5e
  
  3 changesets affected
  84e5416 commit 5
  ff5d556 commit 3
  f548282 commit 1
  3 of 3 chunks applied
  $ hg annotate a
  11: 2b
  12: 4d
  $ hg log -T '{rev} {desc}\n' -Gp
  @  12 commit 4
  │  diff -r 1cae118c7ed8 -r 58a62bade1c6 a
  │  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  │  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  │  @@ -1,1 +1,2 @@
  │   2b
  │  +4d
  │
  o  11 commit 2
  │  diff -r 84add69aeac0 -r 1cae118c7ed8 a
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
  13: insert before 2b
  13: 2b
  14: 4d
  14: insert aftert 4d

# Bookmarks are moved:

  $ hg bookmark -r '.^' b1
  $ hg bookmark -r '.' b2
  $ hg bookmark ba
  $ hg bookmarks
     b1                        b35060a57a50
     b2                        946e4bc87915
   * ba                        946e4bc87915
  $ sed -i 's/insert/INSERT/' a
  $ hg absorb -aq
  $ hg status
  $ hg bookmarks
     b1                        a4183e9b3d31
     b2                        c9b20c925790
   * ba                        c9b20c925790

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
  a4183e9 -INSERT before 2b
  a4183e9 +Insert before 2b
          @@ -3,1 +3,1 @@
  c9b20c9 -INSERT aftert 4d
  c9b20c9 +Insert aftert 4d
  
  2 changesets affected
  c9b20c9 commit 4
  a4183e9 commit 2
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
  85b4e0e -Insert aftert 4d
  85b4e0e +insert aftert 4d
  
  1 changeset affected
  85b4e0e commit 4
  $ hg absorb -a
  showing changes for a
          @@ -0,1 +0,1 @@
          -Insert before 2b
          +insert before 2b
          @@ -3,1 +3,1 @@
  85b4e0e -Insert aftert 4d
  85b4e0e +insert aftert 4d
  
  1 changeset affected
  85b4e0e commit 4
  1 of 2 chunks applied
  $ hg diff -U 0
  diff -r 1c8eadede62a a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -Insert before 2b
  +insert before 2b
  $ hg annotate a
  18: Insert before 2b
  18: 2b
  21: 4d
  21: insert aftert 4d

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
  2517e37 -b line 1
  61782db -b line 2
  2517e37 +b Line 1
  61782db +b Line 2
  
  2 changesets affected
  61782db commit b 2
  2517e37 commit b 1
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
  9:712d16a8f445834e36145408eabc1d29df05ec09 commit b 2
  8:74cfa6294160149d60adbf7582b99ce37a4597ec commit b 1
  5:28f10dcf96158f84985358a2e5d5b3505ca69c22 commit a 2
  4:f9a81da8dc53380ed91902e5b82c1b36255a4bd0 commit a 1
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
  712d16a -b Line 2
          +b line 1
  712d16a +b line 2
  
  1 changeset affected
  712d16a commit b 2

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
  28f10dc -a Line 2
          +a line 1
  28f10dc +a line 2
  showing changes for b
          @@ -0,2 +0,2 @@
  74cfa62 -b Line 1
  712d16a -b Line 2
  74cfa62 +b line 1
  712d16a +b line 2
  
  3 changesets affected
  712d16a commit b 2
  74cfa62 commit b 1
  28f10dc commit a 2
  2 of 2 chunks applied
  $ hg log -T '{rev}:{node|short} {desc} {get(extras, "absorb_source")}\n'
  12:cbc0c676ae8f commit b 2 
  11:071dee819ad0 commit b 1 
  10:4faf555e5598 commit a 2 
  4:f9a81da8dc53 commit a 1 
  $ hg absorb -a
  showing changes for a
          @@ -0,1 +0,1 @@
  f9a81da -a Line 1
  f9a81da +a line 1
  
  1 changeset affected
  f9a81da commit a 1
  1 of 1 chunk applied
  $ hg log -T '{rev}:{node|short} {desc} {get(extras, "absorb_source")}\n'
  3:a478955a9e03 commit b 2 
  2:7380d5e6fab8 commit b 1 
  1:4472dd5179eb commit a 2 
  0:6905bbb02e4e commit a 1 

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
  a478955 +END
  
  1 changeset affected
  a478955 commit b 2
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
  bfafb49242db: 1 file(s) changed, became 259b86984766
  115485984805: 2 file(s) changed, became bd7f2557c265
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
