  $ enable absorb

  $ sedi() { # workaround check-code
  > pattern="$1"
  > shift
  > for i in "$@"; do
  >     sed "$pattern" "$i" > "$i".tmp
  >     mv "$i".tmp "$i"
  > done
  > }

  $ newrepo

Do not crash with empty repo:

  $ hg absorb
  abort: no changeset to change
  [255]

Make some commits:

  $ for i in 1 2 3 4 5; do
  >   echo $i >> a
  >   hg commit -A a -m "commit $i" -q
  > done

  $ hg annotate a
  0: 1
  1: 2
  2: 3
  3: 4
  4: 5

Change a few lines:

  $ cat > a <<EOF
  > 1a
  > 2b
  > 3
  > 4d
  > 5e
  > EOF

Preview absorb changes:

  $ hg absorb --print-changes --dry-run
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

  $ hg absorb --print-changes --print-descriptions --dry-run --config absorb.maxdescwidth=15
  showing changes for a
                           @@ -0,2 +0,2 @@
  4ec16f8 commit 1         -1
  5c5f952 commit 2         -2
  4ec16f8 commit 1         +1a
  5c5f952 commit 2         +2b
                           @@ -3,2 +3,2 @@
  ad8b8b7 commit 4         -4
  4f55fa6 commit 5         -5
  ad8b8b7 commit 4         +4d
  4f55fa6 commit 5         +5e

Run absorb:

  $ hg absorb
  saved backup bundle to * (glob)
  2 of 2 chunk(s) applied
  $ hg annotate a
  0: 1a
  1: 2b
  2: 3
  3: 4d
  4: 5e

Delete a few lines and related commits will be removed if they will be empty:

  $ cat > a <<EOF
  > 2b
  > 4d
  > EOF
  $ hg absorb
  saved backup bundle to * (glob)
  3 of 3 chunk(s) applied
  $ hg annotate a
  1: 2b
  2: 4d
  $ hg log -T '{rev} {desc}\n' -Gp
  @  2 commit 4
  |  diff -r 1cae118c7ed8 -r 58a62bade1c6 a
  |  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -1,1 +1,2 @@
  |   2b
  |  +4d
  |
  o  1 commit 2
  |  diff -r 84add69aeac0 -r 1cae118c7ed8 a
  |  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,1 @@
  |  +2b
  |
  o  0 commit 1
  

Non 1:1 map changes will be ignored:

  $ echo 1 > a
  $ hg absorb
  nothing applied
  [1]

Insertaions:

  $ cat > a << EOF
  > insert before 2b
  > 2b
  > 4d
  > insert aftert 4d
  > EOF
  $ hg absorb -q
  $ hg status
  $ hg annotate a
  1: insert before 2b
  1: 2b
  2: 4d
  2: insert aftert 4d

Bookmarks are moved:

  $ hg bookmark -r 1 b1
  $ hg bookmark -r 2 b2
  $ hg bookmark ba
  $ hg bookmarks
     b1                        1:b35060a57a50
     b2                        2:946e4bc87915
   * ba                        2:946e4bc87915
  $ sedi 's/insert/INSERT/' a
  $ hg absorb -q
  $ hg status
  $ hg bookmarks
     b1                        1:a4183e9b3d31
     b2                        2:c9b20c925790
   * ba                        2:c9b20c925790

Non-mofified files are ignored:

  $ touch b
  $ hg commit -A b -m b
  $ touch c
  $ hg add c
  $ hg rm b
  $ hg absorb
  nothing applied
  [1]
  $ sedi 's/INSERT/Insert/' a
  $ hg absorb
  saved backup bundle to * (glob)
  2 of 2 chunk(s) applied
  $ hg status
  A c
  R b

Public commits will not be changed:

  $ hg phase -p 1
  $ sedi 's/Insert/insert/' a
  $ hg absorb -pn
  showing changes for a
          @@ -0,1 +0,1 @@
          -Insert before 2b
          +insert before 2b
          @@ -3,1 +3,1 @@
  85b4e0e -Insert aftert 4d
  85b4e0e +insert aftert 4d
  $ hg absorb
  saved backup bundle to * (glob)
  1 of 2 chunk(s) applied
  $ hg diff -U 0
  diff -r 1c8eadede62a a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	* (glob)
  @@ -1,1 +1,1 @@
  -Insert before 2b
  +insert before 2b
  $ hg annotate a
  1: Insert before 2b
  1: 2b
  2: 4d
  2: insert aftert 4d

Make working copy clean:

  $ hg revert -q -C a b
  $ hg forget c
  $ rm c
  $ hg status

Merge commit will not be changed:

  $ echo 1 > m1
  $ hg commit -A m1 -m m1
  $ hg bookmark -q -i m1
  $ hg update -q '.^'
  $ echo 2 > m2
  $ hg commit -q -A m2 -m m2
  $ hg merge -q m1
  $ hg commit -m merge
  $ hg bookmark -d m1
  $ hg log -G -T '{rev} {desc} {phase}\n'
  @    6 merge draft
  |\
  | o  5 m2 draft
  | |
  o |  4 m1 draft
  |/
  o  3 b draft
  |
  o  2 commit 4 draft
  |
  o  1 commit 2 public
  |
  o  0 commit 1 public
  
  $ echo 2 >> m1
  $ echo 2 >> m2
  $ hg absorb
  abort: no changeset to change
  [255]
  $ hg revert -q -C m1 m2

Use a new repo:

  $ newrepo

Make some commits to multiple files:

  $ for f in a b; do
  >   for i in 1 2; do
  >     echo $f line $i >> $f
  >     hg commit -A $f -m "commit $f $i" -q
  >   done
  > done

Use pattern to select files to be fixed up:

  $ sedi 's/line/Line/' a b
  $ hg status
  M a
  M b
  $ hg absorb a
  saved backup bundle to * (glob)
  1 of 1 chunk(s) applied
  $ hg status
  M b
  $ hg absorb --exclude b
  nothing applied
  [1]
  $ hg absorb b
  saved backup bundle to * (glob)
  1 of 1 chunk(s) applied
  $ hg status
  $ cat a b
  a Line 1
  a Line 2
  b Line 1
  b Line 2

Test config option absorb.maxstacksize:

  $ sedi 's/Line/line/' a b
  $ hg log -T '{rev}:{node} {desc}\n'
  3:712d16a8f445834e36145408eabc1d29df05ec09 commit b 2
  2:74cfa6294160149d60adbf7582b99ce37a4597ec commit b 1
  1:28f10dcf96158f84985358a2e5d5b3505ca69c22 commit a 2
  0:f9a81da8dc53380ed91902e5b82c1b36255a4bd0 commit a 1
  $ hg --config absorb.maxstacksize=1 absorb -pn
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

Test obsolete markers creation:

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution=createmarkers
  > [absorb]
  > addnoise=1
  > EOF

  $ hg --config absorb.maxstacksize=3 sf
  absorb: only the recent 3 changesets will be analysed
  2 of 2 chunk(s) applied
  $ hg log -T '{rev}:{node|short} {desc} {get(extras, "absorb_source")}\n'
  6:3dfde4199b46 commit b 2 712d16a8f445834e36145408eabc1d29df05ec09
  5:99cfab7da5ff commit b 1 74cfa6294160149d60adbf7582b99ce37a4597ec
  4:fec2b3bd9e08 commit a 2 28f10dcf96158f84985358a2e5d5b3505ca69c22
  0:f9a81da8dc53 commit a 1 
  $ hg absorb
  1 of 1 chunk(s) applied
  $ hg log -T '{rev}:{node|short} {desc} {get(extras, "absorb_source")}\n'
  10:e1c8c1e030a4 commit b 2 3dfde4199b4610ea6e3c6fa9f5bdad8939d69524
  9:816c30955758 commit b 1 99cfab7da5ffdaf3b9fc6643b14333e194d87f46
  8:5867d584106b commit a 2 fec2b3bd9e0834b7cb6a564348a0058171aed811
  7:8c76602baf10 commit a 1 f9a81da8dc53380ed91902e5b82c1b36255a4bd0

Test config option absorb.amendflags and running as a sub command of amend:

  $ cat >> $TESTTMP/dummyamend.py << EOF
  > from mercurial import commands, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('amend', [], '')
  > def amend(ui, repo, *pats, **opts):
  >     return 3
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=$TESTTMP/dummyamend.py
  > [absorb]
  > amendflag = correlated
  > EOF

  $ hg amend -h
  hg amend
  
  (no help text available)
  
  options:
  
    --correlated incorporate corrections into stack. see 'hg help absorb' for
                 details
  
  (some details hidden, use --verbose to show complete help)

  $ $PYTHON -c 'print("".join(map(chr, range(0,3))))' > c
  $ hg commit -A c -m 'c is a binary file'
  $ echo c >> c
  $ sedi $'2i\\\nINS\n' b
  $ echo END >> b
  $ hg rm a
  $ hg amend --correlated
  1 of 2 chunk(s) applied
  
  # changes not applied and left in working directory:
  # M b : 1 modified chunks were ignored
  # M c : unsupported file type (ex. binary or link)
  # R a : removed files were ignored

Executable files:

  $ cat >> $HGRCPATH << EOF
  > [diff]
  > git=True
  > EOF
  $ newrepo
  $ echo > foo.py
  $ chmod +x foo.py
  $ hg add foo.py
  $ hg commit -mfoo

  $ echo bla > foo.py
  $ hg absorb --dry-run --print-changes
  showing changes for foo.py
          @@ -0,1 +0,1 @@
  99b4ae7 -
  99b4ae7 +bla
  $ hg absorb
  1 of 1 chunk(s) applied
  $ hg diff -c .
  diff --git a/foo.py b/foo.py
  new file mode 100755
  --- /dev/null
  +++ b/foo.py
  @@ -0,0 +1,1 @@
  +bla
  $ hg diff

Remove lines may delete changesets:

  $ newrepo
  $ cat > a <<EOF
  > 1
  > 2
  > EOF
  $ hg commit -m a12 -A a
  $ cat > b <<EOF
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
  $ hg absorb -pn
  showing changes for a
          @@ -2,0 +2,1 @@
  bfafb49 +3
  showing changes for b
          @@ -1,3 +1,0 @@
  1154859 -2
  30970db -3
  a393a58 -4
  $ hg absorb -v | grep became
  bfafb49242db: 1 file(s) changed, became 1a2de97fc652
  115485984805: 2 file(s) changed, became 0c930dfab74c
  30970dbf7b40: became empty and was dropped
  a393a58b9a85: became empty and was dropped
  $ hg log -T '{rev} {desc}\n' -Gp
  @  5 b12
  |  diff --git a/b b/b
  |  new file mode 100644
  |  --- /dev/null
  |  +++ b/b
  |  @@ -0,0 +1,1 @@
  |  +1
  |
  o  4 a12
     diff --git a/a b/a
     new file mode 100644
     --- /dev/null
     +++ b/a
     @@ -0,0 +1,3 @@
     +1
     +2
     +3
  
