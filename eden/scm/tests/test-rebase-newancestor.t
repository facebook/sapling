#chg-compatible
#debugruntest-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest
  $ enable rebase
  $ readconfig <<EOF
  > [revsetalias]
  > dev=desc("dev")
  > def=desc("def")
  > EOF

  $ hg init repo
  $ cd repo

  $ echo A > a
  $ echo >> a
  $ hg ci -Am A
  adding a

  $ echo B > a
  $ echo >> a
  $ hg ci -m B

  $ echo C > a
  $ echo >> a
  $ hg ci -m C

  $ hg up -q -C 'desc(A)'

  $ echo D >> a
  $ hg ci -Am AD

  $ tglog
  @  3878212183bd 'AD'
  │
  │ o  30ae917c0e4f 'C'
  │ │
  │ o  0f4f7cb4f549 'B'
  ├─╯
  o  1e635d440a73 'A'
  
  $ hg rebase -s 'desc(B)' -d 'desc(AD)'
  rebasing 0f4f7cb4f549 "B"
  merging a
  rebasing 30ae917c0e4f "C"
  merging a

  $ tglog
  o  25773bc4b4b0 'C'
  │
  o  c09015405f75 'B'
  │
  @  3878212183bd 'AD'
  │
  o  1e635d440a73 'A'
  

  $ cd ..


Test rebasing of merges with ancestors of the rebase destination - a situation
that often happens when trying to recover from repeated merging with a mainline
branch.

The test case creates a dev branch that contains a couple of merges from the
default branch. When rebasing to the default branch, these merges would be
merges with ancestors on the same branch. The merges _could_ contain some
interesting conflict resolutions or additional changes in the merge commit, but
that is mixed up with the actual merge stuff and there is in general no way to
separate them.

Note: The dev branch contains _no_ changes to f-default. It might be unclear
how rebasing of ancestor merges should be handled, but the current behavior
with spurious prompts for conflicts in files that didn't change seems very
wrong.

The branches are emulated using commit messages.

  $ hg init ancestor-merge
  $ cd ancestor-merge

  $ drawdag <<'EOS'
  >        default4
  >        |
  >        | devmerge2
  >        |/|
  > default3 |
  >        | |
  >        | devmerge1
  >        |/|
  > default2 |
  >        | |
  >        | dev2
  >        | |
  >        | dev1
  >        |/
  >        default1
  > EOS
  $ hg clone -qU . ../ancestor-merge-2

Full rebase all the way back from branching point:

  $ hg rebase -r 'only(dev,def)' -d $default4 --config ui.interactive=True << EOF
  > c
  > EOF
  rebasing 1e48f4172d62 "dev1"
  rebasing aeae94a564c6 "dev2"
  rebasing da5b1609fcb1 "devmerge1"
  note: rebase of da5b1609fcb1 created no changes to commit
  rebasing bea5bcfda5f9 "devmerge2"
  note: rebase of bea5bcfda5f9 created no changes to commit
  $ tglog
  o  f66b059fae0f 'dev2'
  │
  o  1073bfc4c1ed 'dev1'
  │
  o  22e5a3eb70f1 'default4'
  │
  o  a51061c4b2cb 'default3'
  │
  o  dfbdae6572c4 'default2'
  │
  o  6ee4113c6616 'default1'
  
Grafty cherry picking rebasing:

  $ cd ../ancestor-merge-2

  $ hg rebase -r 'children(only(dev,def))' -d $default4 --config ui.interactive=True << EOF
  > c
  > EOF
  rebasing aeae94a564c6 "dev2"
  rebasing da5b1609fcb1 "devmerge1"
  note: rebase of da5b1609fcb1 created no changes to commit
  rebasing bea5bcfda5f9 "devmerge2"
  note: rebase of bea5bcfda5f9 created no changes to commit
  $ tglog
  o  9cdc50ee9a9d 'dev2'
  │
  o  22e5a3eb70f1 'default4'
  │
  o  a51061c4b2cb 'default3'
  │
  │ o  1e48f4172d62 'dev1'
  │ │
  o │  dfbdae6572c4 'default2'
  ├─╯
  o  6ee4113c6616 'default1'
  
  $ cd ..


Test order of parents of rebased merged with un-rebased changes as p1.

  $ hg init parentorder
  $ cd parentorder
  $ touch f
  $ hg ci -Aqm common
  $ touch change
  $ hg ci -Aqm change
  $ touch target
  $ hg ci -Aqm target
  $ hg up -qr 'desc(common)'
  $ touch outside
  $ hg ci -Aqm outside
  $ hg merge -qr 'desc(change)'
  $ hg ci -m 'merge p1 3=outside p2 1=ancestor'
  $ hg par
  commit:      6990226659be
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge p1 3=outside p2 1=ancestor
  
  $ hg up -qr 'desc(change)'
  $ hg merge -qr f59da8fc0fcf04d44bb0390e824516ed9979b69f
  $ hg ci -qm 'merge p1 1=ancestor p2 3=outside'
  $ hg par
  commit:      a57575f79074
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge p1 1=ancestor p2 3=outside
  
  $ tglog
  @    a57575f79074 'merge p1 1=ancestor p2 3=outside'
  ├─╮
  │ │ o  6990226659be 'merge p1 3=outside p2 1=ancestor'
  ╭─┬─╯
  │ o  f59da8fc0fcf 'outside'
  │ │
  │ │ o  a60552eb93fb 'target'
  ├───╯
  o │  dd40c13f7a6f 'change'
  ├─╯
  o  02f0f58d5300 'common'
  
  $ hg rebase -r 'desc("p1 3=outside")' -d 'desc(target)'
  rebasing 6990226659be "merge p1 3=outside p2 1=ancestor"
  $ hg tip
  commit:      cca50676b1c5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge p1 3=outside p2 1=ancestor
  
  $ hg rebase -r 'desc("p1 1=ancestor")' -d 'desc(target)'
  rebasing a57575f79074 "merge p1 1=ancestor p2 3=outside"
  $ hg tip
  commit:      f9daf77ffe76
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge p1 1=ancestor p2 3=outside
  
  $ tglog
  @    f9daf77ffe76 'merge p1 1=ancestor p2 3=outside'
  ├─╮
  │ │ o  cca50676b1c5 'merge p1 3=outside p2 1=ancestor'
  ╭─┬─╯
  │ o  f59da8fc0fcf 'outside'
  │ │
  o │  a60552eb93fb 'target'
  │ │
  o │  dd40c13f7a6f 'change'
  ├─╯
  o  02f0f58d5300 'common'
  
rebase of merge of ancestors

  $ hg up -qr 'desc(target)'
  $ hg merge -qr 'desc(outside)-desc(merge)'
  $ echo 'other change while merging future "rebase ancestors"' > other
  $ hg ci -Aqm 'merge rebase ancestors'
  $ hg rebase -d 'desc("merge p1 1")' -v
  rebasing 4c5f12f25ebe "merge rebase ancestors"
  resolving manifests
  removing other
  note: merging f9daf77ffe76+ and 4c5f12f25ebe using bids from ancestors a60552eb93fb and f59da8fc0fcf
  
  calculating bids for ancestor a60552eb93fb
  resolving manifests
  
  calculating bids for ancestor f59da8fc0fcf
  resolving manifests
  
  auction for merging merge bids
   other: consensus for g
  end of auction
  
  getting other
  committing files:
  other
  committing manifest
  committing changelog
  rebase merging completed
  rebase completed
  $ tglog
  @  113755df812b 'merge rebase ancestors'
  │
  o    f9daf77ffe76 'merge p1 1=ancestor p2 3=outside'
  ├─╮
  │ │ o  cca50676b1c5 'merge p1 3=outside p2 1=ancestor'
  ╭─┬─╯
  │ o  f59da8fc0fcf 'outside'
  │ │
  o │  a60552eb93fb 'target'
  │ │
  o │  dd40c13f7a6f 'change'
  ├─╯
  o  02f0f58d5300 'common'
  
Due to the limitation of 3-way merge algorithm (1 merge base), rebasing a merge
may include unwanted content:

  $ hg init $TESTTMP/dual-merge-base1
  $ cd $TESTTMP/dual-merge-base1
  $ hg debugdrawdag <<'EOS'
  >   F
  >  /|
  > D E
  > | |
  > B C
  > |/
  > A Z
  > |/
  > R
  > EOS
  $ hg rebase -r D+E+F -d Z
  rebasing 5f2c926dfecf "D" (D)
  rebasing b296604d9846 "E" (E)
  rebasing caa9781e507d "F" (F)
  abort: rebasing caa9781e507d will include unwanted changes from d6003a550c2c or c1e6b162678d
  [255]

The warning does not get printed if there is no unwanted change detected:

  $ hg init $TESTTMP/dual-merge-base2
  $ cd $TESTTMP/dual-merge-base2
  $ hg debugdrawdag <<'EOS'
  >   D
  >  /|
  > B C
  > |/
  > A Z
  > |/
  > R
  > EOS
  $ hg rebase -r B+C+D -d Z
  rebasing c1e6b162678d "B" (B)
  rebasing d6003a550c2c "C" (C)
  rebasing c8f78076273e "D" (D)
  $ hg manifest -r 'desc(D)'
  B
  C
  R
  Z

The merge base could be different from old p1 (changed parent becomes new p1):

  $ hg init $TESTTMP/chosen-merge-base1
  $ cd $TESTTMP/chosen-merge-base1
  $ hg debugdrawdag <<'EOS'
  >   F
  >  /|
  > D E
  > | |
  > B C Z
  > EOS
  $ hg rebase -r D+F -d Z
  rebasing 004dc1679908 "D" (D)
  rebasing 4be4cbf6f206 "F" (F)
  $ hg manifest -r 'desc(F)'
  C
  D
  E
  Z
  $ hg log -r 'max(desc(F))^' -T '{desc}\n'
  D

  $ hg init $TESTTMP/chosen-merge-base2
  $ cd $TESTTMP/chosen-merge-base2
  $ hg debugdrawdag <<'EOS'
  >   F
  >  /|
  > D E
  > | |
  > B C Z
  > EOS
  $ hg rebase -r E+F -d Z
  rebasing 974e4943c210 "E" (E)
  rebasing 4be4cbf6f206 "F" (F)
  $ hg manifest -r 'desc(F)'
  B
  D
  E
  Z
  $ hg log -r 'max(desc(F))^' -T '{desc}\n'
  E
