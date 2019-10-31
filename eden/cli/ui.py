#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import enum
import sys
from typing import BinaryIO, Dict, Optional, TextIO, Tuple


class Color(enum.Enum):
    RED = enum.auto()
    GREEN = enum.auto()
    YELLOW = enum.auto()


class Attribute(enum.IntFlag):
    BOLD = 0x01
    UNDERLINE = 0x02


class Output(abc.ABC):
    RED = Color.RED
    GREEN = Color.GREEN
    YELLOW = Color.YELLOW
    BOLD = Attribute.BOLD

    def writeln(
        self,
        msg: str,
        fg: Optional[Color] = None,
        bg: Optional[Color] = None,
        attr: Optional[Attribute] = None,
        flush: bool = False,
    ) -> None:
        self.write(msg, fg=fg, bg=bg, attr=attr, end="\n", flush=flush)

    @abc.abstractmethod
    def write(
        self,
        msg: str,
        fg: Optional[Color] = None,
        bg: Optional[Color] = None,
        attr: Optional[Attribute] = None,
        end: Optional[str] = None,
        flush: bool = False,
    ) -> None:
        pass


class PlainOutput(Output):
    def __init__(self, io: TextIO) -> None:
        self.io = io

    def write(
        self,
        msg: str,
        fg: Optional[Color] = None,
        bg: Optional[Color] = None,
        attr: Optional[Attribute] = None,
        end: Optional[str] = None,
        flush: bool = False,
    ) -> None:
        self.io.write(msg)
        if end:
            self.io.write(end)
        if flush:
            self.io.flush()


_term_settings: Optional["TerminalSettings"] = None


class TerminalSettings:
    def __init__(
        self,
        foreground: Dict[Color, bytes],
        background: Dict[Color, bytes],
        attributes: Dict[Attribute, bytes],
        reset: bytes,
    ) -> None:
        self._foreground = foreground
        self._background = background
        self._attributes = attributes
        self._reset = reset

    @staticmethod
    def getinstance() -> "TerminalSettings":
        """Get the TerminalSettings singleton object for this programs TTY.

        This function calls curses.setupterm() to initialize the terminal the first time
        it is called.  Subsequent calls return the previously looked up terminal
        information.
        """
        global _term_settings
        if _term_settings is not None:
            # pyre-fixme[7]: Expected `TerminalSettings` but got
            #  `Optional[TerminalSettings]`.
            return _term_settings

        import curses

        curses.setupterm()

        set_foreground = curses.tigetstr("setaf") or b""
        foreground = {
            Color.RED: curses.tparm(set_foreground, curses.COLOR_RED),
            Color.GREEN: curses.tparm(set_foreground, curses.COLOR_GREEN),
            Color.YELLOW: curses.tparm(set_foreground, curses.COLOR_YELLOW),
        }

        set_background = curses.tigetstr("setab") or b""
        background = {
            Color.RED: curses.tparm(set_background, curses.COLOR_RED),
            Color.GREEN: curses.tparm(set_background, curses.COLOR_GREEN),
            Color.YELLOW: curses.tparm(set_background, curses.COLOR_YELLOW),
        }

        attributes = {
            Attribute.BOLD: curses.tigetstr("bold") or b"",
            Attribute.UNDERLINE: curses.tigetstr("smul") or b"",
        }

        reset = curses.tigetstr("sgr0") or b""

        _term_settings = TerminalSettings(
            foreground=foreground,
            background=background,
            attributes=attributes,
            reset=reset,
        )
        # pyre-fixme[7]: Expected `TerminalSettings` but got
        #  `Optional[TerminalSettings]`.
        return _term_settings

    def get_attr_codes(
        self,
        fg: Optional[Color] = None,
        bg: Optional[Color] = None,
        attr: Optional[Attribute] = None,
    ) -> Tuple[bytes, bytes]:
        start = b""
        if fg:
            start += self._foreground[fg]
        if bg:
            start += self._background[bg]
        if attr:
            for attr_type in Attribute:  # type: ignore
                if attr & int(attr_type):
                    start += self._attributes[attr_type]

        if not start:
            return (b"", b"")
        return (start, self._reset)


class TerminalOutput(Output):
    def __init__(
        self, io: BinaryIO, term_settings: TerminalSettings, encoding: str = "utf-8"
    ) -> None:
        self.io = io
        self.term_settings = term_settings
        self.encoding = encoding
        self.encode_error = "replace"

    def write(
        self,
        msg: str,
        fg: Optional[Color] = None,
        bg: Optional[Color] = None,
        attr: Optional[Attribute] = None,
        end: Optional[str] = None,
        flush: bool = False,
    ) -> None:
        start_str, end_str = self.term_settings.get_attr_codes(fg=fg, bg=bg, attr=attr)

        self.io.write(start_str)
        self.io.write(msg.encode(self.encoding, errors=self.encode_error))
        self.io.write(end_str)
        if end:
            self.io.write(end.encode(self.encoding, errors=self.encode_error))
        if flush:
            self.io.flush()


def get_output(io: Optional[TextIO] = None) -> Output:
    if io is None:
        io = sys.stdout
    if not io.isatty():
        return PlainOutput(io)
    io_buffer = getattr(io, "buffer", None)
    if io_buffer is None:
        return PlainOutput(io)

    if sys.platform == "win32":
        from . import win_ui

        return win_ui.WindowsOutput(io)

    import curses

    try:
        encoding = getattr(io, "encoding", "utf-8")
        return TerminalOutput(io_buffer, TerminalSettings.getinstance(), encoding)
    except curses.error:
        # If curses fails for any reason (most likely the user has a broken terminal
        # setting or terminfo database) fall back to the plain output.
        return PlainOutput(io)
