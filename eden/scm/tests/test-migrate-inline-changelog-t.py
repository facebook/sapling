# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig format.inline-changelog=1 visibility.enabled=true mutation.enabled=true"
sh % "newrepo"
sh % "echo A" | "hg debugdrawdag"
sh % 'hg dbsh -c "ui.write(str(repo.svfs.exists(\\"00changelog.d\\")))"' == "False"

sh % "setconfig format.inline-changelog=0"
sh % 'hg log -r tip -T "{desc}\\n"' == r"""
    (migrating to non-inlined changelog)
    A"""

sh % 'hg dbsh -c "ui.write(str(repo.svfs.exists(\\"00changelog.d\\")))"' == "True"
