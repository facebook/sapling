#debugruntest-compatible
# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# Verify ancestry data is readable by mergedrivers by looking at mergestate:

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ newrepo
  $ enable rebase
  $ setconfig 'experimental.evolution='
  $ setconfig 'rebase.singletransaction=True'
  $ setconfig 'rebase.experimental.inmemory=True'

  $ mkdir driver
  $ cat > driver/__init__.py << 'EOF'
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     unresolved_files = list(mergestate.unresolved())
  >     ui.warn("ancestor nodes = %s\n" % [ctx.hex() for ctx in mergestate.ancestorctxs])
  >     ui.warn("ancestor revs = %s\n" % [ctx.rev() for ctx in mergestate.ancestorctxs])
  >     mergestate.commit()
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     pass
  > EOF

  $ setconfig 'experimental.mergedriver=python:driver/'
  $ hg commit -Aqm driver
  $ hg debugdrawdag << 'EOS'
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
  rebasing 19c6d3b0d8fb "A" (A)
  rebasing 5a83467e1fc3 "B" (B)
  rebasing 09810f6b52c0 "F" (F)
  rebasing 3ff755c5931b "C" (C)
  rebasing dc7f2675f9ab "D" (D)
  rebasing 5eb863826611 "E" (E)
  $ showgraph
  o  17085bf4ec19 E
  │
  o  50e74e386d1a D
  │
  o  68805fc8068c C
  │
  │ o  25a05a650d8b F
  ├─╯
  o  0b21084cb212 B
  │
  o  57315db76057 A
  │
  │ o  520a9f665f6e b
  │
  @  2563cf1728bf driver
  $ hg rebase -r 25a05a650d8b -d 17085bf4ec19
  rebasing 25a05a650d8b "F" (F)
  ancestor nodes = ['0b21084cb21221e8ac6138fc5e92460d37525d21']
  ancestor revs = [9]
  merging file
