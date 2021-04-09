# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
amend=
rebase=
[experimental]
evolution = obsolete
[mutation]
enabled=true
record=false
[visibility]
enabled=true
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"

# verify template options

sh % "hg commit --config 'ui.allowemptycommit=True' --template '{desc}\\n' -m 'some commit'" == "some commit"

sh % "hg commit --config 'ui.allowemptycommit=True' --template '{node}\\n' -m 'some commit'" == "15312f872b9e54934cd96e0db83e24aaefc2356d"

sh % "hg commit --config 'ui.allowemptycommit=True' --template '{node|short} ({phase}): {desc}\\n' -m 'some commit'" == "e3bf63af66d6 (draft): some commit"

sh % "echo hello" > "hello.txt"
sh % "hg add hello.txt"

sh % "hg amend --template '{node|short} ({phase}): {desc}\\n'" == "4a5cb78b8fc9 (draft): some commit"

sh % "echo 'good luck'" > "hello.txt"

sh % "hg amend --template '{node|short} ({phase}): {desc}\\n' --to 4a5cb78b8fc9" == r"""
    abort: --to cannot be used with any other options
    [255]"""
sh % "hg commit --amend --template '{node|short} ({phase}): {desc}\\n'" == "1d0c24f9beeb (draft): some commit"
