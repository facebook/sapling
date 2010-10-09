  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > 
  > [alias]
  > tglog = log -G --template "{rev}: '{desc}' {branches}\n"
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
  saved backup bundle to $TESTTMP/b/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  3: 'L1'
  |
  o  2: 'R1'
  |
  o  1: 'C2'
  |
  o  0: 'C1'
  
Re-run:

  $ hg pull --rebase
  pulling from $TESTTMP/a
  searching for changes
  no changes found


Invoke pull --rebase and nothing to rebase:

  $ cd ../c

  $ hg pull --rebase
  pulling from $TESTTMP/a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  nothing to rebase
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg tglog -l 1
  @  2: 'R1'
  |

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

  $ hg tglog -l 1
  o  2: 'R1'
  |

