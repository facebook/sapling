  $ setconfig extensions.treemanifest=!
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > histedit=
  > 
  > [alias]
  > tglog = log -G --template "{rev}: {node|short} '{desc}' {branches}\n"
  > EOF


  $ hg init a
  $ cd a

  $ echo C1 > C1
  $ hg ci -Am C1
  adding C1

  $ echo C2 > C2
  $ hg ci -Am C2
  adding C2

  $ cd ..

  $ hg clone a b
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg clone a c
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd b

  $ echo L1 > L1
  $ hg ci -Am L1
  adding L1


  $ cd ../a

  $ echo R1 > R1
  $ hg ci -Am R1
  adding R1


  $ cd ../b

Now b has one revision to be pulled from a:

  $ hg pull --rebase
  pulling from $TESTTMP/a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 77ae9631bcca
  rebasing 2:ff8d69a621f9 "L1"

  $ tglog
  @  4: d80cc2da061e 'L1'
  |
  o  3: 77ae9631bcca 'R1'
  |
  o  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  
Re-run:

  $ hg pull --rebase
  pulling from $TESTTMP/a
  searching for changes
  no changes found

Abort pull early if working dir is not clean:

  $ echo L1-mod > L1
  $ hg pull --rebase
  abort: uncommitted changes
  (cannot pull with rebase: please commit or shelve your changes first)
  [255]
  $ hg update --clean --quiet

Abort pull early if another operation (histedit) is in progress:

  $ hg histedit . -q --commands - << EOF
  > edit d80cc2da061e histedit: generate unfinished state
  > EOF
  Editing (d80cc2da061e), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ hg pull --rebase
  abort: histedit in progress
  (use 'hg histedit --continue' or 'hg histedit --abort')
  [255]
  $ hg histedit --abort --quiet

Abort pull early with pending uncommitted merge:

  $ cd ..
  $ hg clone --noupdate c d
  $ cd d
  $ tglog
  o  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  
  $ hg update --quiet 0
  $ echo M1 > M1
  $ hg commit --quiet -Am M1
  $ hg update --quiet 1
  $ hg merge 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg pull --rebase
  abort: outstanding uncommitted merge
  (cannot pull with rebase: please commit or shelve your changes first)
  [255]
  $ hg update --clean --quiet

Invoke pull --rebase and nothing to rebase:

  $ cd ../c

  $ hg book norebase
  $ hg pull --rebase
  pulling from $TESTTMP/a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 77ae9631bcca
  nothing to rebase - updating instead
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updating bookmark norebase

  $ tglog -l 1
  @  2: 77ae9631bcca 'R1' norebase
  |
  ~

pull --rebase --update should ignore --update:

  $ hg pull --rebase --update
  pulling from $TESTTMP/a
  searching for changes
  no changes found

pull --rebase doesn't update if nothing has been pulled:

  $ hg up -q 1

  $ hg pull --rebase
  pulling from $TESTTMP/a
  searching for changes
  no changes found

  $ tglog -l 1
  o  2: 77ae9631bcca 'R1' norebase
  |
  ~

  $ cd ..

pull --rebase works when a specific revision is pulled (issue3619)

  $ cd a
  $ tglog
  @  2: 77ae9631bcca 'R1'
  |
  o  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  
  $ echo R2 > R2
  $ hg ci -Am R2
  adding R2
  $ echo R3 > R3
  $ hg ci -Am R3
  adding R3
  $ cd ../c
  $ tglog
  o  2: 77ae9631bcca 'R1' norebase
  |
  @  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  
  $ echo L1 > L1
  $ hg ci -Am L1
  adding L1
  $ hg pull --rev tip --rebase
  pulling from $TESTTMP/a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  new changesets 31cd3a05214e:770a61882ace
  rebasing 3:ff8d69a621f9 "L1"
  $ tglog
  @  6: 518d153c0ba3 'L1'
  |
  o  5: 770a61882ace 'R3'
  |
  o  4: 31cd3a05214e 'R2'
  |
  o  2: 77ae9631bcca 'R1' norebase
  |
  o  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  
pull --rebase works with bundle2 turned on

  $ cd ../a
  $ echo R4 > R4
  $ hg ci -Am R4
  adding R4
  $ tglog
  @  5: 00e3b7781125 'R4'
  |
  o  4: 770a61882ace 'R3'
  |
  o  3: 31cd3a05214e 'R2'
  |
  o  2: 77ae9631bcca 'R1'
  |
  o  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  
  $ cd ../c
  $ hg pull --rebase
  pulling from $TESTTMP/a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 00e3b7781125
  rebasing 6:518d153c0ba3 "L1"
  $ tglog
  @  8: 0d0727eb7ce0 'L1'
  |
  o  7: 00e3b7781125 'R4'
  |
  o  5: 770a61882ace 'R3'
  |
  o  4: 31cd3a05214e 'R2'
  |
  o  2: 77ae9631bcca 'R1' norebase
  |
  o  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  

pull --rebase only update if there is nothing to rebase

  $ cd ../a
  $ echo R5 > R5
  $ hg ci -Am R5
  adding R5
  $ tglog
  @  6: 88dd24261747 'R5'
  |
  o  5: 00e3b7781125 'R4'
  |
  o  4: 770a61882ace 'R3'
  |
  o  3: 31cd3a05214e 'R2'
  |
  o  2: 77ae9631bcca 'R1'
  |
  o  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  
  $ cd ../c
  $ echo L2 > L2
  $ hg ci -Am L2
  adding L2
  $ hg up 'desc(L1)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg pull --rebase
  pulling from $TESTTMP/a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 88dd24261747
  rebasing 8:0d0727eb7ce0 "L1"
  rebasing 9:c1f58876e3bf "L2"
  $ tglog
  o  12: 6dc0ea5dcf55 'L2'
  |
  @  11: 864e0a2d2614 'L1'
  |
  o  10: 88dd24261747 'R5'
  |
  o  7: 00e3b7781125 'R4'
  |
  o  5: 770a61882ace 'R3'
  |
  o  4: 31cd3a05214e 'R2'
  |
  o  2: 77ae9631bcca 'R1' norebase
  |
  o  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  

pull --rebase update (no rebase) use proper update:

- warn about other head.

  $ cd ../a
  $ echo R6 > R6
  $ hg ci -Am R6
  adding R6
  $ cd ../c
  $ hg up 'desc(R5)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg pull --rebase
  pulling from $TESTTMP/a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 65bc164c1d9b
  nothing to rebase - updating instead
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated to "65bc164c1d9b: R6"
  1 other heads for branch "default"
  $ tglog
  @  13: 65bc164c1d9b 'R6'
  |
  | o  12: 6dc0ea5dcf55 'L2'
  | |
  | o  11: 864e0a2d2614 'L1'
  |/
  o  10: 88dd24261747 'R5'
  |
  o  7: 00e3b7781125 'R4'
  |
  o  5: 770a61882ace 'R3'
  |
  o  4: 31cd3a05214e 'R2'
  |
  o  2: 77ae9631bcca 'R1' norebase
  |
  o  1: 783333faa078 'C2'
  |
  o  0: 05d58a0c15dd 'C1'
  
