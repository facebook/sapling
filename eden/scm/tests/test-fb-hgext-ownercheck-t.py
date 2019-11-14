# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
ownercheck=
""" >> "$HGRCPATH"

# ownercheck does not prevent normal hg operations

sh % "hg init repo1"

# make os.getuid return a different, fake uid

sh % "cat" << r"""
import os
_getuid = os.getuid
def fakeuid(): return _getuid() + 1
os.getuid = fakeuid
""" >> "fakeuid.py"

# ownercheck prevents wrong user from creating new repos

sh % "hg --config 'extensions.fakeuid=fakeuid.py' init repo2" == r"""
    abort: $TESTTMP is owned by *, not you * (glob)
    you are likely doing something wrong.
    (you can skip the check using --config extensions.ownercheck=!)
    [255]"""

# ownercheck prevents wrong user from accessing existing repos

sh % "hg --config 'extensions.fakeuid=fakeuid.py' log --repo repo1" == r"""
    abort: $TESTTMP/repo1 is owned by *, not you * (glob)
    you are likely doing something wrong.
    (you can skip the check using --config extensions.ownercheck=!)
    [255]"""
