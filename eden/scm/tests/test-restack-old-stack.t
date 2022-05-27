#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Using modern setup

  $ enable remotenames amend rebase
  $ setconfig experimental.narrow-heads=true visibility.enabled=true mutation.record=true mutation.enabled=true "mutation.date=0 0" experimental.evolution= remotenames.rename.default=remote

# Test restack behavior with old stacks.

  $ newrepo
  $ drawdag << 'EOS'
  >   D2  # amend: D1 -> D2
  >  /    # (This suggests a rebase from E1 to D2)
  > M
  > | E1
  > | |
  > | D1
  > | |
  > | | C1
  > | |/
  > | B1
  > |/
  > | B2  # amend: B1 -> B2
  > |/    # (This suggests a rebase from C1 to B2)
  > A
  > EOS
  $ hg debugremotebookmark master "$M"
  $ hg up -q "$D2"

# Restack should not rebase C1 to B2, since the user is not on the B2 stack.

  $ hg rebase --restack
  rebasing 87d9afc4bc4e "E1"
