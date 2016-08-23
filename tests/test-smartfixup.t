  $ extpath=`dirname $TESTDIR`
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > smartfixup=$extpath/hgext3rd/smartfixup.py
  > EOF

  $ sedi() { # workaround check-code
  > pattern="$1"
  > shift
  > for i in "$@"; do
  >     sed "$pattern" "$i" > "$i".tmp
  >     mv "$i".tmp "$i"
  > done
  > }

  $ hg init repo1
  $ cd repo1

Do not crash with empty repo:

  $ hg sf
  abort: no changset to change
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
  $ hg smartfixup
  saved backup bundle to * (glob)
  2 of 2 chunks(s) applied
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
  $ hg smartfixup
  saved backup bundle to * (glob)
  3 of 3 chunks(s) applied
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
  $ hg smartfixup
  nothing applied
  [1]

Insertaions:

  $ cat > a << EOF
  > insert before 2b
  > 2b
  > 4d
  > insert aftert 4d
  > EOF
  $ hg smartfixup -q
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
  $ hg smartfixup -q
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
  $ hg smartfixup
  nothing applied
  [1]
  $ sedi 's/INSERT/Insert/' a
  $ hg smartfixup
  saved backup bundle to * (glob)
  2 of 2 chunks(s) applied
  $ hg status
  A c
  R b

Public commits will not be changed:

  $ hg phase -p 1
  $ sedi 's/Insert/insert/' a
  $ hg smartfixup
  saved backup bundle to * (glob)
  1 of 2 chunks(s) applied
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
  $ hg smartfixup
  abort: no changset to change
  [255]
  $ hg revert -q -C m1 m2

Use a new repo:

  $ cd ..
  $ hg init repo2
  $ cd repo2

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
  $ hg smartfixup a
  saved backup bundle to * (glob)
  1 of 1 chunks(s) applied
  $ hg status
  M b
  $ hg smartfixup --exclude b
  nothing applied
  [1]
  $ hg smartfixup b
  saved backup bundle to * (glob)
  1 of 1 chunks(s) applied
  $ hg status
  $ cat a b
  a Line 1
  a Line 2
  b Line 1
  b Line 2

Test config option smartfixup.maxstacksize:

  $ sedi 's/Line/line/' a b
  $ hg log -T '{rev}:{node} {desc}\n'
  3:712d16a8f445834e36145408eabc1d29df05ec09 commit b 2
  2:74cfa6294160149d60adbf7582b99ce37a4597ec commit b 1
  1:28f10dcf96158f84985358a2e5d5b3505ca69c22 commit a 2
  0:f9a81da8dc53380ed91902e5b82c1b36255a4bd0 commit a 1
  $ hg --config smartfixup.maxstacksize=1 smartfixup -pn
  smartfixup: only the recent 1 changesets will be analysed
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
  > [smartfixup]
  > addnoise=1
  > EOF

  $ hg --config smartfixup.maxstacksize=3 sf
  smartfixup: only the recent 3 changesets will be analysed
  2 of 2 chunks(s) applied
  $ hg log -T '{rev}:{node|short} {desc} {get(extras, "smartfixup_source")}\n'
  6:812fad2a366c commit b 2 712d16a8f445834e36145408eabc1d29df05ec09
  5:851732d1c4d4 commit b 1 74cfa6294160149d60adbf7582b99ce37a4597ec
  4:4438fcf42c60 commit a 2 28f10dcf96158f84985358a2e5d5b3505ca69c22
  0:f9a81da8dc53 commit a 1 
  $ hg sf
  1 of 1 chunks(s) applied
  $ hg log -T '{rev}:{node|short} {desc} {get(extras, "smartfixup_source")}\n'
  10:1eaff5b07eb1 commit b 2 812fad2a366c62756eafd8e6b45d9e2f27985d11
  9:9354aeb6e762 commit b 1 851732d1c4d433cdd984d6b295158224b81dd717
  8:568249511984 commit a 2 4438fcf42c600562ce2e74062b0a8ad7d246573f
  7:e56aca308c01 commit a 1 f9a81da8dc53380ed91902e5b82c1b36255a4bd0
