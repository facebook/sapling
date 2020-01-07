# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init t"
sh % "cd t"
sh % "echo a" > "a"
sh % "hg add a"
sh % "hg commit -m test"
sh % "rm .hg/requires"
sh % "hg tip" == r"""
    abort: legacy dirstate implementations are no longer supported!
    [255]"""
sh % "echo indoor-pool" > ".hg/requires"
sh % "hg tip" == r"""
    abort: repository requires features unknown to this Mercurial: indoor-pool!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
sh % "echo outdoor-pool" >> ".hg/requires"
sh % "hg tip" == r"""
    abort: repository requires features unknown to this Mercurial: indoor-pool outdoor-pool!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
sh % "cd .."

# Test checking between features supported locally and ones required in
# another repository of push/pull/clone on localhost:

sh % "mkdir supported-locally"
sh % "cd supported-locally"

sh % "hg init supported"
sh % "echo a" > "supported/a"
sh % "hg -R supported commit -Am '#0 at supported'" == "adding a"

sh % "echo featuresetup-test" >> "supported/.hg/requires"
sh % "cat" << r"""
from __future__ import absolute_import
from edenscm.mercurial import extensions, localrepo
def featuresetup(ui, supported):
    for name, module in extensions.extensions(ui):
        if __name__ == module.__name__:
            # support specific feature locally
            supported |= {'featuresetup-test'}
            return
def uisetup(ui):
    localrepo.localrepository.featuresetupfuncs.add(featuresetup)
""" > "$TESTTMP/supported-locally/supportlocally.py"
sh % "cat" << r"""
[extensions]
# enable extension locally
supportlocally = $TESTTMP/supported-locally/supportlocally.py
""" > "supported/.hg/hgrc"
sh % "hg -R supported status"

sh % "hg init push-dst"
sh % "hg -R supported push push-dst" == r"""
    pushing to push-dst
    abort: required features are not supported in the destination: featuresetup-test
    [255]"""

sh % "hg init pull-src"
sh % "hg -R pull-src pull supported" == r"""
    pulling from supported
    abort: required features are not supported in the destination: featuresetup-test
    [255]"""

sh % "hg clone supported clone-dst" == r"""
    abort: repository requires features unknown to this Mercurial: featuresetup-test!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
sh % "hg clone --pull supported clone-dst" == r"""
    abort: required features are not supported in the destination: featuresetup-test
    [255]"""

sh % "cd .."
