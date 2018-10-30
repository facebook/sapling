Verify ancestry data is readable by mergedrivers by looking at mergestate:

  $ newrepo
  $ enable rebase
  $ setconfig experimental.evolution=
  $ setconfig rebase.singletransaction=True
  $ setconfig rebase.experimental.inmemory=True

  $ mkdir driver
  $ cat > driver/__init__.py <<EOF
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     unresolved_files = list(mergestate.unresolved())
  >     ui.warn("ancestor nodes = %s\n" % [ctx.hex() for ctx in mergestate.ancestorctxs])
  >     ui.warn("ancestor revs = %s\n" % [ctx.rev() for ctx in mergestate.ancestorctxs])
  >     mergestate.commit()
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     pass
  > EOF

  $ setconfig experimental.mergedriver=python:driver/
  $ hg commit -Aqm "driver"
  $ hg debugdrawdag <<'EOS'
  > E    # E/file = 1\n2\n3\n4\n5
  > |
  > D
  > |
  > C F b  # F/file = 0\n1\n2\n3\n4
  > |/
  > B
  > |
  > A   # A/file = 1\n2\n3\n4
  > EOS
  $ hg rebase -s A -d 0
  rebasing 1:19c6d3b0d8fb "A" (A)
  rebasing 3:5a83467e1fc3 "B" (B)
  rebasing 5:09810f6b52c0 "F" (F)
  rebasing 4:3ff755c5931b "C" (C)
  rebasing 6:dc7f2675f9ab "D" (D)
  rebasing 7:5eb863826611 "E" (E tip)
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/19c6d3b0d8fb-a2cf1ad8-rebase.hg
  $ showgraph
  o  7 e71547946f82 E
  |
  o  6 264c021e8fc6 D
  |
  o  5 34e41e21cd9d C
  |
  | o  4 aa431a9572c1 F
  |/
  o  3 01ba3ad89eb7 B
  |
  o  2 622e2d864a27 A
  |
  | o  1 520a9f665f6e b
  |
  @  0 9309aa3b805a driver
  $ hg rebase -r aa431a9572c1 -d e71547946f82
  rebasing 4:aa431a9572c1 "F"
  ancestor nodes = ['01ba3ad89eb70070d81f052c0c40a3877c2ba5d8']
  ancestor revs = [3]
  merging file
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/aa431a9572c1-13824be1-rebase.hg
