# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

try:
    from eden.fs.cli.doctor.facebook.lib.fake_vscode_extensions_checker import (
        FakeVSCodeExtensionsChecker,
    )

    def getFakeVSCodeExtensionsChecker() -> FakeVSCodeExtensionsChecker:
        return FakeVSCodeExtensionsChecker()

except ImportError:

    def getFakeVSCodeExtensionsChecker() -> None:
        return None
