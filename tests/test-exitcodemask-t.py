# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Command line flag is effective:

sh % "hg add a --config 'ui.exitcodemask=63'" == r"""
    abort: no repository found in '$TESTTMP' (.hg not found)!
    [63]"""

sh % "'HGPLAIN=1' hg add a --config 'ui.exitcodemask=63'" == r"""
    abort: no repository found in '$TESTTMP' (.hg not found)!
    [63]"""

# Config files are ignored if HGPLAIN is set:

sh % "setconfig 'ui.exitcodemask=31'"
sh % "hg add a" == r"""
    abort: no repository found in '$TESTTMP' (.hg not found)!
    [31]"""

sh % "'HGPLAIN=1' hg add a" == r"""
    abort: no repository found in '$TESTTMP' (.hg not found)!
    [255]"""

# But HGPLAINEXCEPT can override the behavior:

sh % "'HGPLAIN=1' 'HGPLAINEXCEPT=exitcode' hg add a" == r"""
    abort: no repository found in '$TESTTMP' (.hg not found)!
    [31]"""
