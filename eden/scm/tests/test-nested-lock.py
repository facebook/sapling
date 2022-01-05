# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["no-windows"])


def test_nested_lock():
    from edenscm.mercurial import hg, ui as uimod

    ui = uimod.ui.load()
    repo1 = hg.repository(ui, testtmp.TESTTMP, create=True)
    repo2 = hg.repository(ui, testtmp.TESTTMP)
    # repo2.lock() should detect deadlock.
    try:
        with repo1.lock(), repo2.lock(wait=False):
            pass
    except Exception as ex:
        msg = str(ex)
        assert "deadlock" in msg


test_nested_lock()
