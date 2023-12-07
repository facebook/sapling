#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ setconfig experimental.rebase-long-labels=True

  $ eagerepo
  $ enable undo rebase

  $ hg init a
  $ cd a
  $ echo c1 >common
  $ hg add common
  $ hg ci -m C1

  $ echo c2 >>common
  $ hg ci -m C2

  $ echo c3 >>common
  $ hg ci -m C3

  $ hg up -q -C 'desc(C2)'

  $ echo l1 >>extra
  $ hg add extra
  $ hg ci -m L1

  $ sed -e 's/c2/l2/' common > common.new
  $ mv common.new common
  $ hg ci -m L2

  $ echo l3 >> extra2
  $ hg add extra2
  $ hg ci -m L3
  $ hg bookmark mybook

  $ tglogp
  @  8029388f38dc draft 'L3' mybook
  │
  o  46f0b057b5c0 draft 'L2'
  │
  o  3163e20567cc draft 'L1'
  │
  │ o  a9ce13b75fb5 draft 'C3'
  ├─╯
  o  11eb9c356adf draft 'C2'
  │
  o  178f1774564f draft 'C1'
Try to call --continue:

  $ hg rebase --continue
  abort: no rebase in progress
  [255]

Conflicting rebase:

  $ hg rebase -s 'desc(L1)' -d 'desc(C3)'
  rebasing 3163e20567cc "L1"
  rebasing 46f0b057b5c0 "L2"
  merging common
  warning: 1 conflicts while merging common! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg status --config commands.status.verbose=1
  M common
  ? common.orig
  # The repository is in an unfinished *rebase* state.
  
  # Unresolved merge conflicts:
  # 
  #     common
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  
  # To continue:                hg rebase --continue
  # To abort:                   hg rebase --abort

Try to continue without solving the conflict:

  $ hg rebase --continue
  abort: unresolved merge conflicts (see 'hg help resolve')
  [255]

Conclude rebase:

  $ echo 'resolved merge' >common
  $ hg resolve -m common
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg continue
  already rebased 3163e20567cc "L1" as 3e046f2ecedb
  rebasing 46f0b057b5c0 "L2"
  rebasing 8029388f38dc "L3" (mybook)

  $ tglogp
  @  d67b21408fc0 draft 'L3' mybook
  │
  o  5e5bd08c7e60 draft 'L2'
  │
  o  3e046f2ecedb draft 'L1'
  │
  o  a9ce13b75fb5 draft 'C3'
  │
  o  11eb9c356adf draft 'C2'
  │
  o  178f1774564f draft 'C1'
Check correctness:

  $ hg cat -r 'desc(C1)' common
  c1

  $ hg cat -r 'desc(C2)' common
  c1
  c2

  $ hg cat -r 'desc(C3)' common
  c1
  c2
  c3

  $ hg cat -r 'desc(L1)' common
  c1
  c2
  c3

  $ hg cat -r 'desc(L2)' common
  resolved merge

  $ hg cat -r 'desc(L3)' common
  resolved merge

Bookmark stays active after --continue
  $ hg bookmarks
   * mybook                    d67b21408fc0

  $ cd ..

Test minimization of merge conflicts
  $ newrepo
  $ echo a > a
  $ hg add a
  $ hg commit -q -m 'a'
  $ echo b >> a
  $ hg commit -q -m 'ab'
  $ hg bookmark ab
  $ hg up -q '.^'
  $ echo b >> a
  $ echo c >> a
  $ hg commit -q -m 'abc'
  $ hg rebase -s 7bc217434fc1 -d ab --keep
  rebasing 7bc217434fc1 "abc"
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg diff
  diff -r 328e4ab1f7cc a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	* (glob)
  @@ -1,2 +1,6 @@
   a
   b
  +<<<<<<< dest (rebasing onto):    328e4ab1f7cc ab - test: ab
  +=======
  +c
  +>>>>>>> source (being rebased):  7bc217434fc1 - test: abc
  $ hg rebase --abort
  rebase aborted
  $ hg up -q -C 7bc217434fc1
  $ hg rebase -s . -d ab --keep -t internal:merge3
  rebasing 7bc217434fc1 "abc"
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg diff
  diff -r 328e4ab1f7cc a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	* (glob)
  @@ -1,2 +1,8 @@
   a
  +<<<<<<< dest (rebasing onto):    328e4ab1f7cc ab - test: ab
   b
  +||||||| base (parent of source): cb9a9f314b8b - test: a
  +=======
  +b
  +c
  +>>>>>>> source (being rebased):  7bc217434fc1 - test: abc

Test rebase with obsstore turned on and off (issue5606)

  $ cd $TESTTMP
  $ hg init b
  $ cd b
  $ hg debugdrawdag <<'EOS'
  > D
  > |
  > C
  > |
  > B E
  > |/
  > A
  > EOS

  $ hg goto E -q
  $ echo 3 > B
  $ hg commit --amend -m E -A B -q
  $ hg rebase -r B+D -d . --config experimental.evolution=true
  rebasing 112478962961 "B" (B)
  merging B
  warning: 1 conflicts while merging B! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ echo 4 > B
  $ hg resolve -m
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue --config experimental.evolution=none
  rebasing 112478962961 "B" (B)
  rebasing f585351a92f8 "D" (D)

  $ tglogp
  o  c5f6f5f52dbd draft 'D' D
  │
  o  a8990ee99807 draft 'B' B
  │
  @  b2867df0c236 draft 'E' E
  │
  │ o  26805aba1e60 draft 'C' C
  │ │
  │ x  112478962961 draft 'B'
  ├─╯
  o  426bada5c675 draft 'A' A
