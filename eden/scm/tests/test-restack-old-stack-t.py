# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Using modern setup

sh % "enable remotenames amend rebase"
sh % 'setconfig experimental.narrow-heads=true visibility.enabled=true mutation.record=true mutation.enabled=true "mutation.date=0 0" experimental.evolution= remotenames.rename.default=remote'

# Test restack behavior with old stacks.

sh % "newrepo"
sh % "drawdag" << r"""
  D2  # amend: D1 -> D2
 /    # (This suggests a rebase from E1 to D2)
M
| E1
| |
| D1
| |
| | C1
| |/
| B1
|/
| B2  # amend: B1 -> B2
|/    # (This suggests a rebase from C1 to B2)
A
"""
sh % 'hg debugremotebookmark master "$M"'
sh % 'hg up -q "$D2"'

# Restack should not rebase C1 to B2, since the user is not on the B2 stack.

sh % "hg rebase --restack" == 'rebasing 87d9afc4bc4e "E1"'
