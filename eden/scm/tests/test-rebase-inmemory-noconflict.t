#require no-eden

#testcases single-transaction multi-transaction

#if single-transaction
  $ setconfig rebase.singletransaction=true
#else
  $ setconfig rebase.singletransaction=false
#endif

  $ configure mutation-norecord
Tests the --noconflict rebase flag

  $ enable amend morestatus rebase
  $ setconfig morestatus.show=True
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig rebase.experimental.inmemorywarning="rebasing in-memory!"
  $ configure modern
  $ newclientrepo

  $ hg debugdrawdag <<'EOS'
  > c
  > |
  > b g
  > |/
  > a
  > EOS
  $ hg up -q g
  $ echo "conflict" > c
  $ hg add -q
  $ hg amend -q
  $ hg up c
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (changing active bookmark from g to c)
  $ echo "local change" > b


Confirm it fails when rebasing a change that conflicts:
  $ hg rebase -r tip -d . --noconflict
  rebasing in-memory!
  rebasing 955ac081fc7c "g" (g)
  merging c
  hit merge conflicts (in c) and --noconflict passed; exiting
  $ hg st
  M b
  $ cat b
  local change

Confirm rebase without a merge behaves the same:
  $ hg rebase -r tip -d .~1 --noconflict
  rebasing in-memory!
  rebasing 955ac081fc7c "g" (g)

Confirm the flag fails without IMM:

  $ setconfig rebase.experimental.inmemory=False
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  $ hg rebase -r tip -d . --noconflict
  abort: --noconflict requires in-memory merge
  [255]

Confirm that it rebases a three-way merge, but no conflict:
  $ newclientrepo
  $ seq 1 5 > a
  $ hg commit -Aq -m "base"
  $ seq 1 10 > a
  $ hg commit -q -m "extend to 10"
  $ hg up -q .~1
  $ seq 0 5 > a
  $ hg commit -q -m "prepend with 0"
  $ hg log -G -r 'desc(base)':: -T '{desc}'
  @  prepend with 0
  │
  │ o  extend to 10
  ├─╯
  o  base
  
  $ hg up -qC 'desc(base)'
  $ hg rebase -r 'desc(extend)' -d 'desc(prepend)' --noconflict
  rebasing in-memory!
  rebasing 12cba56c6d27 "extend to 10"
  merging a
  $ hg cat -r tip a | wc -l
  11


Test state of non-conflicting parts of stack after conflict:

  $ newclientrepo
  $ drawdag <<EOS
  >   D  # D/B = conflict
  >   |
  > B C
  > |/
  > A
  > EOS
  $ hg rebase -s $C -d $B --noconflict
  rebasing in-memory!
  rebasing dc0947a82db8 "C"
  rebasing afeb1d864871 "D"
  merging B
  hit merge conflicts (in B) and --noconflict passed; exiting

#if single-transaction
Single transaction undoes everything:
  $ tglog
  o  afeb1d864871 'D'
  │
  o  dc0947a82db8 'C'
  │
  │ o  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'
#else
Multi-transaction leaves partial rebase reults:
  $ tglog
  o  bbfdd6cb49aa 'C'
  │
  │ o  afeb1d864871 'D'
  │ │
  │ x  dc0947a82db8 'C'
  │ │
  o │  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'
#endif
