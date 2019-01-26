#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import io

import eden.cli.ui


class TestOutput(eden.cli.ui.TerminalOutput):
    def __init__(self) -> None:
        Color = eden.cli.ui.Color
        Attribute = eden.cli.ui.Attribute
        term_settings = eden.cli.ui.TerminalSettings(
            foreground={
                Color.RED: b"<red>",
                Color.GREEN: b"<green>",
                Color.YELLOW: b"<yellow>",
            },
            background={
                Color.RED: b"<red_bg>",
                Color.GREEN: b"<green_bg>",
                Color.YELLOW: b"<yellow_bg>",
            },
            attributes={Attribute.BOLD: b"<bold>", Attribute.UNDERLINE: b"<underline>"},
            reset=b"<reset>",
        )
        self._out = io.BytesIO()
        super().__init__(self._out, term_settings)

    def getvalue(self) -> str:
        return self._out.getvalue().decode("utf-8", errors="surrogateescape")
