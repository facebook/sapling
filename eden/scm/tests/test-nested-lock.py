# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


def test_nested_lock():
    import os, tempfile

    from edenscm import hg, ui as uimod

    if os.name == "nt":
        return

    with tempfile.TemporaryDirectory() as t:
        os.chdir(t)
        ui = uimod.ui.load()
        repo1 = hg.repository(ui, t, create=True)
        repo2 = hg.repository(ui, t)
        # repo2.lock() should detect deadlock.
        try:
            with repo1.lock(), repo2.lock(wait=False):
                pass
        except Exception as ex:
            msg = str(ex)
            assert "deadlock" in msg


if __name__ == "__main__":
    test_nested_lock()
