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
  $ echo c1 >common
  $ hg add common
  $ hg ci -m C1

  $ echo c2 >>common
  $ hg ci -m C2

  $ echo c3 >>common
  $ hg ci -m C3

  $ hg up -q -C 1

  $ echo l1 >>extra
  $ hg add extra
  $ hg ci -m L1
  created new head

  $ sed -e 's/c2/l2/' common > common.new
  $ mv common.new common
  $ hg ci -m L2

  $ echo l3 >> extra2
  $ hg add extra2
  $ hg ci -m L3

  $ hg tglog
  @  5: 'L3'
  |
  o  4: 'L2'
  |
  o  3: 'L1'
  |
  | o  2: 'C3'
  |/
  o  1: 'C2'
  |
  o  0: 'C1'
  
Try to call --continue:

  $ hg rebase --continue
  abort: no rebase in progress
  [255]

Conflicting rebase:

  $ hg rebase -s 3 -d 2
  merging common
  warning: conflicts during merge.
  merging common failed!
  abort: unresolved conflicts (see hg resolve, then hg rebase --continue)
  [255]

Try to continue without solving the conflict:

  $ hg rebase --continue 
  abort: unresolved merge conflicts (see hg help resolve)
  [255]

Conclude rebase:

  $ echo 'resolved merge' >common
  $ hg resolve -m common
  $ hg rebase --continue
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  5: 'L3'
  |
  o  4: 'L2'
  |
  o  3: 'L1'
  |
  o  2: 'C3'
  |
  o  1: 'C2'
  |
  o  0: 'C1'
  
Check correctness:

  $ hg cat -r 0 common
  c1

  $ hg cat -r 1 common
  c1
  c2

  $ hg cat -r 2 common
  c1
  c2
  c3

  $ hg cat -r 3 common
  c1
  c2
  c3

  $ hg cat -r 4 common
  resolved merge

  $ hg cat -r 5 common
  resolved merge

