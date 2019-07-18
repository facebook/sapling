# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["no-fsmonitor"])

sh % "hg init a"
sh % "cd a"
sh % "hg init b"
sh % "echo x" > "b/x"

# Should print nothing:

sh % "hg add b"
sh % "hg st"

sh % "echo y" > "b/y"
sh % "hg st"

# Should fail:

sh % "hg st b/x" == r"""
    abort: path 'b/x' is inside nested repo 'b'
    [255]"""
sh % "hg add b/x" == r"""
    abort: path 'b/x' is inside nested repo 'b'
    [255]"""

# Should fail:

sh % "hg add b b/x" == r"""
    abort: path 'b/x' is inside nested repo 'b'
    [255]"""
sh % "hg st"

# Should arguably print nothing:

sh % "hg st b"

sh % "echo a" > "a"
sh % "hg ci -Ama a"

# Should fail:

sh % "hg mv a b" == r"""
    abort: path 'b/a' is inside nested repo 'b'
    [255]"""
sh % "hg st"

sh % "cd .."
