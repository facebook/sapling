#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import sys
from typing import Optional


class AnsiEscapeCodes:
    __slots__ = ('bold', 'red', 'green', 'yellow', 'reset')

    def __init__(
        self, bold: str, red: str, green: str, yellow: str, reset: str
    ) -> None:
        self.bold = bold
        self.red = red
        self.green = green
        self.yellow = yellow
        self.reset = reset


class StdoutPrinter:

    def __init__(self, escapes: Optional[AnsiEscapeCodes] = None) -> None:
        if escapes is not None:
            self._bold = escapes.bold
            self._red = escapes.red
            self._green = escapes.green
            self._yellow = escapes.yellow
            self._reset = escapes.reset
        elif sys.stdout.isatty():
            import curses

            curses.setupterm()
            self._bold = (curses.tigetstr('bold') or b'').decode()
            set_foreground = curses.tigetstr('setaf') or b''
            self._red = curses.tparm(set_foreground, curses.COLOR_RED).decode()
            self._green = curses.tparm(set_foreground, curses.COLOR_GREEN).decode()
            self._yellow = curses.tparm(set_foreground, curses.COLOR_YELLOW).decode()
            self._reset = (curses.tigetstr('sgr0') or b'').decode()
        else:
            self._bold = ''
            self._red = ''
            self._green = ''
            self._yellow = ''
            self._reset = ''

    def bold(self, text: str) -> str:
        return self._bold + text + self._reset

    def red(self, text: str) -> str:
        return self._red + text + self._reset

    def green(self, text: str) -> str:
        return self._green + text + self._reset

    def yellow(self, text: str) -> str:
        return self._yellow + text + self._reset
