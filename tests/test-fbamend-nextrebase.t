Set up test environment.
  $ . $TESTDIR/require-ext.sh directaccess evolve inhibit
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/fbamend.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > directaccess=
  > evolve=
  > fbamend=$TESTTMP/fbamend.py
  > inhibit=
  > rebase=
  > strip=
  > [experimental]
  > evolution = createmarkers
  > evolutioncommands = prev next
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    hg ci -l msg
  > }

Create a situation where child commits are left behind after amend.
  $ hg init nextrebase && cd nextrebase
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1] add b
  $ echo "b2" > b2
  $ hg add b2
  $ hg amend -m "add b and b2"
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg log --graph -T "{rev} {desc|firstline}"
  @  4 add b and b2
  |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  

Check to ensure hg rebase --next works.
  $ hg next --rebase --dry-run
  hg rebase -r 4538525df7e2b9f09423636c61ef63a4cb872a2d -d 29509da8015c02a5a44d703e561252f6478a1430 -k
  hg next
  $ hg next --rebase
  rebasing 2:4538525df7e2 "add c"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [5] add c
  $ hg log --graph -T "{rev} {desc|firstline}"
  @  5 add c
  |
  o  4 add b and b2
  |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  

Check whether it works with multiple children.
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg strip 4+5
  saved backup bundle to $TESTTMP/nextrebase/.hg/strip-backup/29509da8015c-5bd88862-backup.hg (glob)
  $ mkcommit d
  created new head
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1] add b
  $ echo "b3" > b3
  $ hg add b3
  $ hg amend -m "add b and b3"
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg log --graph -T "{rev} {desc|firstline}"
  @  6 add b and b3
  |
  | o  4 add d
  | |
  | | o  2 add c
  | |/
  | o  1 add b
  |/
  o  0 add a
  

  $ hg next --rebase
  rebasing 2:4538525df7e2 "add c"
  rebasing 4:78f83396d79e "add d"
  ambigious next changeset:
  [7] add c
  [8] add d
  explicitly update to one of them
  [1]
  $ hg log --graph -T "{rev} {desc|firstline}"
  o  8 add d
  |
  | o  7 add c
  |/
  @  6 add b and b3
  |
  | o  4 add d
  | |
  | | o  2 add c
  | |/
  | o  1 add b
  |/
  o  0 add a
  

Check whether hg next --rebase behaves correctly when there is a conflict.
  $ mkcommit conflict
  created new head
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [6] add b and b3
  $ echo "different" > conflict
  $ hg add conflict
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg log --graph -T "{rev} {desc|firstline}"
  @  11 add b and b3
  |
  | o  9 add conflict
  | |
  | | o  8 add d
  | |/
  | | o  7 add c
  | |/
  | o  6 add b and b3
  |/
  | o  4 add d
  | |
  | | o  2 add c
  | |/
  | o  1 add b
  |/
  o  0 add a
  

  $ hg next --rebase
  rebasing 2:4538525df7e2 "add c"
  rebasing 4:78f83396d79e "add d"
  rebasing 7:2e88ee75f11f "add c"
  rebasing 8:1a847fbbfbb6 "add d"
  rebasing 9:b8431585b2c3 "add conflict"
  merging conflict
  warning: conflicts while merging conflict! (edit, then use 'hg resolve --mark')
  please resolve any conflicts, run 'hg rebase --continue', and then run 'hg next'
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg next --rebase
  abort: rebase in progress
  (use 'hg rebase --continue' or 'hg rebase --abort')
  [255]
  $ rm conflict
  $ echo "merged" > conflict
  $ hg resolve --mark conflict
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  already rebased 2:4538525df7e2 "add c" as 5e344ef92cb2
  already rebased 4:78f83396d79e "add d" as 97c68b3e05f1
  already rebased 7:2e88ee75f11f "add c" as 198890c29490
  already rebased 8:1a847fbbfbb6 "add d" as ccdc055b4afe
  rebasing 9:b8431585b2c3 "add conflict"
  $ hg log --graph -T "{rev} {desc|firstline}"
  o  16 add conflict
  |
  | o  15 add d
  |/
  | o  14 add c
  |/
  | o  13 add d
  |/
  | o  12 add c
  |/
  @  11 add b and b3
  |
  | o  9 add conflict
  | |
  | | o  8 add d
  | |/
  | | o  7 add c
  | |/
  | o  6 add b and b3
  |/
  | o  4 add d
  | |
  | | o  2 add c
  | |/
  | o  1 add b
  |/
  o  0 add a
  
