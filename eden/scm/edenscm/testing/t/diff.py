# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import difflib
import os
import re
from typing import Callable, List, Optional, Tuple


class ExpectLine:
    r"""an "expected" line with hints (glob) (re) (esc) (feature !) (?)
    (?) - The line is optional.
    (feature !) - The line only exist if feature is present
    (esc) - The line contains escape sequences (ex. \r)
    (glob) - The line contains glob patterns (ex. '*')
    (re) - The line is a regular expression
    """
    # raw line content, including the hints, without ending "\n"
    rawline: str

    # (?)
    optional: bool = False

    # (feature !) and feature is not present
    excluded: bool = False

    # (glob) or (esc)
    isre: bool = False

    # the line body without the hints
    body: str = ""

    def __init__(
        self, rawline: str, hasfeature: Optional[Callable[[str], bool]] = None
    ):
        optional = False
        excluded = False
        isre = False
        body = rawline.rstrip("\n")

        if body.endswith(" (?)"):
            body = body[:-4]
            optional = True
        if body.endswith(" !)") and " (" in body:
            body, rest = body.rsplit(" (", 1)
            if hasfeature is None:
                optional = True
            else:
                feature = rest.split(" ", 1)[0]
                if hasfeature(feature):
                    optional = False
                else:
                    optional = True
                    excluded = True
        if body.endswith(" (esc)"):
            body = unescape(body[:-6])
        # normalize path separator on windows
        if os.name == "nt":
            body = body.replace("\\", "/")
        if body.endswith(" (glob)"):
            # translate to re pattern
            isre = True
            body = body[:-7]
            body = re.escape(body).replace(r"\*", ".*").replace(r"\?", ".")
        elif body.endswith(" (re)"):
            isre = True
            body = body[:-5]

        self.rawline = rawline
        self.optional = optional
        self.excluded = excluded
        self.isre = isre
        self.body = body

    def match(self, line: str) -> bool:
        """match an output line (without hints)"""
        # normalize path separator on windows
        if os.name == "nt":
            line = line.replace("\\", "/")
        if self.isre:
            # pyre-fixme[7]: Expected `bool` but got `Optional[Match[str]]`.
            return re.match(self.body + r"\Z", line)
        else:
            return self.body == line

    def __repr__(self) -> str:
        components = [repr(self.body)]
        if self.isre:
            components.append("(re)")
        if self.excluded:
            components.append("(false !)")
        elif self.optional:
            components.append("(?)")
        return f"<ExpectLine {' '.join(components)}>"


class MultiLineMatcher:
    r"""Multi-line matcher with special hints understanding.

    See ExpectLine for what kind of hints are supported.

    Compare plain text with trailing space normalization:

        >>> m = MultiLineMatcher('')
        >>> m.match('')
        True
        >>> m.match('\n')
        True
        >>> m.match('a')
        False

        >>> m = MultiLineMatcher('a\n')
        >>> m.match("a")
        True
        >>> m.match("a\n\n")
        True
        >>> m.match("b")
        False

    Pattern matching:

        >>> m = MultiLineMatcher('a* (glob)\n[ab] (re)')
        >>> m.elines
        [<ExpectLine 'a.*' (re)>, <ExpectLine '[ab]' (re)>]

        >>> m.match('a1\na\n')
        True
        >>> m.normalize('a1\nb\n')[0]
        'a* (glob)\n[ab] (re)\n'
        >>> m.normalize('c\nb\n')[0]
        'c\n[ab] (re)\n'

    Optional lines:

        >>> m = MultiLineMatcher('a\nb* (glob) (?)\nc\n')
        >>> m.elines
        [<ExpectLine 'a'>, <ExpectLine 'b.*' (re) (?)>, <ExpectLine 'c'>]

        >>> m.match('a\nc\n')
        True
        >>> m.match('a\nb\nc\n')
        True
        >>> m.match('a\nb\nb\nc\n')
        False

        >>> m.normalize('a\nc\n')[0]
        'a\nb* (glob) (?)\nc\n'
        >>> m.normalize('a\nb\nc\n')[0]
        'a\nb* (glob) (?)\nc\n'

    Feature-gated lines:

        >>> m = MultiLineMatcher('a\nb (foo !)\nc (bar !)\n', lambda f: f == 'bar')
        >>> m.elines
        [<ExpectLine 'a'>, <ExpectLine 'b' (false !)>, <ExpectLine 'c'>]

        >>> m.match('a\nc')
        True
        >>> m.normalize('a\nc')[0]
        'a\nb (foo !)\nc (bar !)\n'

        >>> m.match('a')
        False
        >>> m.match('a\nb\nc')
        False

    "..." matches multiple lines:

        >>> m = MultiLineMatcher('a\n...\ne\n')
        >>> m.match('a\nb\nc\nd\ne\n')
        True
        >>> m.normalize('a\nb\nc\nd\ne\nf\n')[0]
        'a\n...\ne\nf\n'
        >>> m.match('a\ne\n')
        True
        >>> m.normalize('a\ne\n')[0]
        'a\n...\ne\n'

    """

    def __init__(
        self, expected: str, hasfeature: Optional[Callable[[str], bool]] = None
    ):
        self.blines = blines = splitlines(expected.rstrip("\n"))
        self.elines: List[ExpectLine] = [ExpectLine(l, hasfeature) for l in blines]
        self._cache = {}

    def match(self, actual: str) -> bool:
        """Test if actual output (without hints) match the expected lines."""
        return self._matchandnormalizecached(actual)[0]

    def normalize(self, actual: str) -> Tuple[str, str]:
        """Normalzie (actual, expected) for plain text diff.

        Return (actual, expected) pair, which is more friendly for a plain
        text diff algorithm.
        """
        # pyre-fixme[7]: Expected `Tuple[str, str]` but got `Tuple[Union[bool, str],
        #  ...]`.
        return self._matchandnormalizecached(actual)[1:]

    def _matchandnormalizecached(self, actual: str) -> Tuple[bool, str, str]:
        result = self._cache.get(actual)
        if result is None:
            result = self._cache[actual] = self._matchandnormalize(actual)
        return result

    def _matchandnormalize(self, actual: str) -> Tuple[bool, str, str]:
        alines = splitlines(actual.rstrip("\n"))
        blines = self.blines
        elines = self.elines

        # alines including hints without affecting match result so plain text
        # diff with b is cleaner
        glines: List[str] = []

        matched = True

        for (tag, i1, i2, j1, j2) in difflib.SequenceMatcher(
            a=alines, b=blines
        ).get_opcodes():
            if tag == "equal":
                glines += alines[i1:i2]
            elif tag == "delete":
                glines += alines[i1:i2]
                matched = False
            elif tag == "replace" or tag == "insert":
                # "..." matches multiple lines (similar to stdlib doctest)
                if blines[j1:j2] == ["..."]:
                    glines.append(blines[j1])
                    continue
                # naively trying to match with glob patterns
                # (this for loop is empty for tag == "insert")
                j = j1
                for i in range(i1, i2):
                    while j < j2 and elines[j].excluded:
                        glines.append(blines[j])
                        j += 1
                    while (
                        j < j2 and not elines[j].match(alines[i]) and elines[j].optional
                    ):
                        glines.append(blines[j])
                        j += 1
                    if j < j2 and elines[j].match(alines[i]):
                        glines.append(blines[j])
                    else:
                        glines.append(alines[i])
                        matched = False
                    j += 1
                    i += 1
                for j in range(j, j2):
                    if elines[j].optional:
                        glines.append(blines[j])
                    else:
                        matched = False

        a = "".join(l + "\n" for l in glines)
        b = "".join(l + "\n" for l in blines)
        return (matched, a, b)


def splitlines(s: str) -> List[str]:
    r"""split lines by \r or \n, and escape \r lines

    Examples:

        >>> splitlines('a\rb\r')
        ['a\\r (no-eol) (esc)', 'b\\r (esc)']

        >>> splitlines('a\r\nb\r')
        ['a\\r (esc)', 'b\\r (esc)']
    """
    if not s:
        return []
    lines = s.split("\n")
    if "\r" in s:
        # For compatibility, "\r" is handled specially:
        # - "\r" lines are escaped to "\\r" suffixed by " (esc)"
        # - "\r" in the middle of a line is split into 2 lines:
        #   ex. "a\rb" becomes "a\\r (no-eol) (esc)\nb"
        newlines = []
        for line in lines:
            if "\r" in line:
                suffix = " (no-eol) (esc)\n"
                line = line.replace("\r", f"\\r{suffix}")
                if line.endswith(suffix):
                    line = line[: -len(suffix)] + " (esc)"
                newlines += line.split("\n")
            else:
                newlines.append(line)
        lines = newlines
    return lines


def unescape(s: str) -> str:
    r"""replace \x... \n \r with raw bytes and decode as utf8

    Examples:

        >>> unescape(r'a\n\\\tb')
        'a\n\\\tb'
        >>> unescape(r'\xe5\xad\x97')
        '字'

    """
    buf = []
    nextch = iter(s).__next__
    while True:
        try:
            ch = nextch()
        except StopIteration:
            break
        if ch == "\\":
            fmt = nextch()
            if fmt == "x":
                hex1 = nextch()
                hex2 = nextch()
                buf.append(bytes([int(f"{hex1}{hex2}", 16)]))
            elif fmt == "r":
                buf.append(b"\r")
            elif fmt == "n":
                buf.append(b"\n")
            elif fmt == "t":
                buf.append(b"\t")
            elif fmt == "\\":
                buf.append(b"\\")
            else:
                raise ValueError(f"unknown escape {repr(fmt)} in: {s}")
        else:
            buf.append(ch.encode())
    return b"".join(buf).decode()
