# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Create a treedirstate repo

sh % "hg init repo1 --config 'format.dirstate=1'"
sh % "cd repo1"
sh % "touch x"
sh % "hg ci -m init -A x"

# Set the size field to -1:

sh % 'hg debugshell --command \'with repo.wlock(), repo.lock(), repo.transaction("dirstate") as tr: repo.dirstate.normallookup("x"); repo.dirstate.write(tr)\''
sh % "hg debugstate" == "n   0         -1 unset               x"

# Upgrade to v2 does not turn "n" into "m":

sh % "hg debugtree v2"
sh % "hg debugstate" == "n   0         -1 unset               x"
