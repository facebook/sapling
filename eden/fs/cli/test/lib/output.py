#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import io

import eden.fs.cli.ui


class TestOutput(eden.fs.cli.ui.TerminalOutput):
    def __init__(self) -> None:
        Color = eden.fs.cli.ui.Color
        Attribute = eden.fs.cli.ui.Attribute
        term_settings = eden.fs.cli.ui.TerminalSettings(
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
