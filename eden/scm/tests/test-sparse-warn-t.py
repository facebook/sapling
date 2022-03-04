# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "enable sparse"
sh % "newrepo"
sh % "touch file"
sh % "hg commit -Aqm 'add file'"

sh % "setconfig sparse.warnfullcheckout=hint"
sh % "hg status" == r"""
    hint[sparse-fullcheckout]: warning: full checkouts will eventually be disabled in this repository. Use EdenFS or hg sparse to get a smaller repository.
    hint[hint-ack]: use 'hg hint --ack sparse-fullcheckout' to silence these hints"""

sh % "setconfig sparse.warnfullcheckout=warn"
sh % "hg status" == "warning: full checkouts will soon be disabled in this repository. Use EdenFS or hg sparse to get a smaller repository."

sh % "setconfig sparse.warnfullcheckout=softblock"
sh % "hg status" == r"""
    abort: full checkouts are not supported for this repository
    (use EdenFS or hg sparse)
    [255]"""

sh % "setconfig sparse.bypassfullcheckoutwarn=True"
sh % "hg status" == "warning: full checkouts will soon be disabled in this repository. Use EdenFS or hg sparse to get a smaller repository."

sh % "setconfig sparse.warnfullcheckout=hardblock"
sh % "hg status" == r"""
    abort: full checkouts are not supported for this repository
    (use EdenFS or hg sparse)
    [255]"""

sh % "hg sparse include file" == ""
sh % "hg status" == ""
