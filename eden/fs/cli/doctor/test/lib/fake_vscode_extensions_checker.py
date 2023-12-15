# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import List

try:
    from eden.fs.cli.doctor.facebook.lib.fake_vscode_extensions_checker import (
        FakeVSCodeExtensionsChecker,
    )

    def getFakeVSCodeExtensionsChecker() -> FakeVSCodeExtensionsChecker:
        return FakeVSCodeExtensionsChecker(None)

    def getFakeVSCodeExtensionsCheckerWithExtensions(
        extensions: List[str],
    ) -> FakeVSCodeExtensionsChecker:
        return FakeVSCodeExtensionsChecker(extensions)

except ImportError:

    def getFakeVSCodeExtensionsChecker() -> None:
        return None

    def getFakeVSCodeExtensionsCheckerWithExtensions(extensions: List[str]) -> None:
        return None
