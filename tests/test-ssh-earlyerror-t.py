# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg --config 'ui.ssh=echo ssh: SSH is not working 1>&2; exit 1;' clone 'ssh://foo//bar'" == r"""
    ssh: SSH is not working
    abort: no suitable response from remote hg!
    [255]"""
