# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import io
import re
from dataclasses import dataclass
from typing import Any

from . import crecord, error


_MAX_INITIAL_HUNK_LINES = 100
_MAX_INITIAL_FILE_LINES = 500


class SelectorError(ValueError):
    pass


@dataclass(frozen=True)
class Selector:
    file_start: int
    file_end: int
    hunk_start: int | None = None
    hunk_end: int | None = None
    line_start: int | None = None
    line_end: int | None = None

    def __str__(self) -> str:
        result = _formatrange("F", self.file_start, self.file_end)
        if self.hunk_start is not None:
            result += "." + _formatrange("H", self.hunk_start, self.hunk_end)
        if self.line_start is not None:
            result += "." + _formatrange("L", self.line_start, self.line_end)
        return result


def _formatrange(prefix: str, start: int, end: int | None) -> str:
    if end is None or end == start:
        return f"{prefix}{start}"
    return f"{prefix}{start}-{end}"


def _parserange(start: str, end: str | None, selector: str) -> tuple[int, int]:
    first = int(start)
    last = int(end) if end is not None else first
    if first == 0 or last < first:
        raise SelectorError(f"invalid range in selector: {selector}")
    return first, last


def parseselector(text: str) -> Selector:
    """Parse one self-contained file, hunk, or changed-line selector.

    Ranges are allowed only on the final component, so each selector remains
    meaningful when copied out of a comma-separated selector set.

    >>> str(parseselector("f1.h2.l3-10"))
    'F1.H2.L3-10'
    >>> str(parseselector("F4-5"))
    'F4-5'
    >>> parseselector("F1-2.H3")
    Traceback (most recent call last):
    ...
    sapling.repl_record.SelectorError: ranges are only allowed on the final component: F1-2.H3
    """
    match = re.fullmatch(
        r"F([0-9]+)(?:-([0-9]+))?"
        r"(?:\.H([0-9]+)(?:-([0-9]+))?"
        r"(?:\.L([0-9]+)(?:-([0-9]+))?)?)?",
        text,
        re.IGNORECASE,
    )
    if match is None:
        raise SelectorError(f"invalid selector: {text}")

    file_start, file_end = _parserange(match[1], match[2], text)
    if match[3] is None:
        return Selector(file_start, file_end)
    if file_start != file_end:
        raise SelectorError(f"ranges are only allowed on the final component: {text}")

    hunk_start, hunk_end = _parserange(match[3], match[4], text)
    if match[5] is None:
        return Selector(file_start, file_end, hunk_start, hunk_end)
    if hunk_start != hunk_end:
        raise SelectorError(f"ranges are only allowed on the final component: {text}")

    line_start, line_end = _parserange(match[5], match[6], text)
    return Selector(
        file_start,
        file_end,
        hunk_start,
        hunk_end,
        line_start,
        line_end,
    )


def parseselectors(text: str) -> list[Selector]:
    """Parse the comma-separated selector syntax accepted by REPL commands.

    >>> [str(value) for value in parseselectors(
    ...     "F1.H1.L1-100,F1.H2.L3-10,F4-5"
    ... )]
    ['F1.H1.L1-100', 'F1.H2.L3-10', 'F4-5']
    """
    parts = [part.strip() for part in text.split(",")]
    if not parts or any(not part for part in parts):
        raise SelectorError("expected a comma-separated selector set")
    return [parseselector(part) for part in parts]


def _selectablelines(hunk: Any) -> list[Any]:
    return [
        line for line in hunk.changedlines if line.prettystr().startswith((b"+", b"-"))
    ]


class _PatchIndex:
    def __init__(self, headers: list[Any]) -> None:
        self.headers = headers
        self.hunks: list[Any] = []
        self.lines: dict[tuple[int, int], list[Any]] = {}
        self.locations: dict[int, tuple[int, int | None, int | None]] = {}
        self.trailingmarkers: dict[int, Any] = {}

        for fileno, header in enumerate(headers, 1):
            self.locations[id(header)] = (fileno, None, None)
            if header.special():
                continue
            for hunkno, hunk in enumerate(header.hunks, 1):
                self.hunks.append(hunk)
                self.locations[id(hunk)] = (fileno, hunkno, None)
                selectable = _selectablelines(hunk)
                self.lines[(fileno, hunkno)] = selectable
                for lineno, line in enumerate(selectable, 1):
                    self.locations[id(line)] = (fileno, hunkno, lineno)
                for position, line in enumerate(hunk.changedlines[:-1]):
                    marker = hunk.changedlines[position + 1]
                    if marker.prettystr().startswith(b"\\"):
                        self.trailingmarkers[id(line)] = marker

    def resolve(self, selector: Selector) -> list[Any]:
        files = self._slice(self.headers, selector.file_start, selector.file_end, "F")
        if selector.hunk_start is None:
            return files

        header = files[0]
        if header.special():
            raise SelectorError(f"F{selector.file_start} is an atomic file change")
        hunks = self._slice(
            header.hunks,
            selector.hunk_start,
            selector.hunk_end,
            f"F{selector.file_start}.H",
        )
        if selector.line_start is None:
            return hunks

        lines = self.lines[(selector.file_start, selector.hunk_start)]
        return self._slice(
            lines,
            selector.line_start,
            selector.line_end,
            f"F{selector.file_start}.H{selector.hunk_start}.L",
        )

    def resolveall(self, selectors: list[Selector]) -> list[Any]:
        result = []
        seen = set()
        for selector in selectors:
            for node in self.resolve(selector):
                if id(node) not in seen:
                    result.append(node)
                    seen.add(id(node))
        return result

    def withmarkers(self, nodes: list[Any]) -> list[Any]:
        result = list(nodes)
        for node in nodes:
            marker = self.trailingmarkers.get(id(node))
            if marker is not None:
                result.append(marker)
        return result

    @staticmethod
    def _slice(items: list[Any], start: int, end: int | None, label: str) -> list[Any]:
        last = end if end is not None else start
        if last > len(items):
            raise SelectorError(
                f"{_formatrange(label, start, last)} is out of range; "
                f"valid range is {label}1-{len(items)}"
            )
        return items[start - 1 : last]


def _state(node: Any) -> str:
    if getattr(node, "partial", False):
        return "[~]"
    return "[x]" if node.applied else "[ ]"


def _hunkheader(hunk: Any) -> bytes:
    output = io.BytesIO()
    hunk._hunk.write(output)
    return output.getvalue().splitlines(keepends=True)[0]


def _hunklinecount(hunk: Any) -> int:
    return len(hunk.before) + len(hunk.changedlines) + len(hunk.after)


def _renderhunk(
    ui: Any,
    hunk: Any,
    fileno: int,
    hunkno: int,
    collapsed: bool = False,
    showhint: bool = True,
) -> None:
    label = f"F{fileno}.H{hunkno}"
    patchlinecount = _hunklinecount(hunk)
    ui.write(f"  {_state(hunk)} {label} ")
    ui.writebytes(_hunkheader(hunk))
    if collapsed:
        if showhint:
            ui.write(f"      {patchlinecount} lines hidden; use 'show {label}'\n")
        return

    linecount = sum(
        line.prettystr().startswith((b"+", b"-")) for line in hunk.changedlines
    )
    linelabelwidth = len(f"{label}.L{linecount}")
    contextprefix = b" " * (len("    [ ] ") + linelabelwidth + 1)
    for line in hunk.before:
        ui.writebytes(contextprefix + line)

    lineno = 0
    for line in hunk.changedlines:
        text = line.prettystr()
        if text.startswith((b"+", b"-")):
            lineno += 1
            linelabel = f"{label}.L{lineno}"
            prefix = f"    {_state(line)} {linelabel:<{linelabelwidth}} ".encode()
        else:
            prefix = contextprefix
        ui.writebytes(prefix + text)
    for line in hunk.after:
        ui.writebytes(contextprefix + line)


def _renderheader(
    ui: Any,
    header: Any,
    fileno: int,
    hunknumbers: set[int] | None = None,
    foldlong: bool = False,
) -> None:
    added = sum(hunk._hunk.added for hunk in header.hunks)
    removed = sum(hunk._hunk.removed for hunk in header.hunks)
    ui.write(
        f"{_state(header)} F{fileno} {header.filename()!r} (+{added} -{removed})\n"
    )
    if header.special():
        ui.writebytes(header.prettystr())
        return
    filelinecount = sum(_hunklinecount(hunk) for hunk in header.hunks)
    filecollapsed = foldlong and filelinecount > _MAX_INITIAL_FILE_LINES
    if filecollapsed:
        ui.write(
            f"  {filelinecount} lines hidden; use 'show F{fileno}' or a hunk selector\n"
        )
    for hunkno, hunk in enumerate(header.hunks, 1):
        if hunknumbers is None or hunkno in hunknumbers:
            hunkcollapsed = filecollapsed or (
                foldlong and _hunklinecount(hunk) > _MAX_INITIAL_HUNK_LINES
            )
            _renderhunk(
                ui,
                hunk,
                fileno,
                hunkno,
                collapsed=hunkcollapsed,
                showhint=not filecollapsed,
            )


def _rendernodes(
    ui: Any, index: _PatchIndex, nodes: list[Any], foldlong: bool = False
) -> None:
    fullfiles = set()
    hunksbyfile: dict[int, set[int]] = {}
    for node in nodes:
        fileno, hunkno, _lineno = index.locations[id(node)]
        if hunkno is None:
            fullfiles.add(fileno)
        else:
            hunksbyfile.setdefault(fileno, set()).add(hunkno)

    for fileno, header in enumerate(index.headers, 1):
        if fileno in fullfiles:
            _renderheader(ui, header, fileno, foldlong=foldlong)
        elif fileno in hunksbyfile:
            _renderheader(ui, header, fileno, hunksbyfile[fileno], foldlong=foldlong)


def _writestatus(ui: Any, index: _PatchIndex) -> None:
    lines = [line for values in index.lines.values() for line in values]
    fullfiles = sum(header.applied and not header.partial for header in index.headers)
    partialfiles = sum(header.partial for header in index.headers)
    fullhunks = sum(hunk.applied and not hunk.partial for hunk in index.hunks)
    partialhunks = sum(hunk.partial for hunk in index.hunks)
    filepartial = f" (+{partialfiles} partial)" if partialfiles else ""
    hunkpartial = f" (+{partialhunks} partial)" if partialhunks else ""
    ui.write(
        "Selection: "
        f"files {fullfiles}/{len(index.headers)}{filepartial}, "
        f"hunks {fullhunks}/{len(index.hunks)}{hunkpartial}, "
        f"lines {sum(line.applied for line in lines)}/{len(lines)}\n"
    )


def _writepreview(ui: Any, headers: list[Any]) -> None:
    chunks = crecord.selectedchunks(headers)
    if not chunks:
        ui.write("(empty selection)\n")
        return
    output = io.BytesIO()
    for chunk in chunks:
        chunk.write(output)
    ui.writebytes(output.getvalue())


_HELP = """Commands:
  select SELECTORS    select files, hunks, or changed lines
  deselect SELECTORS  deselect files, hunks, or changed lines
  show SELECTORS      show original patches with selection state
  status              show selection counts
  preview             show the exact selected patch
  confirm             finish selection
  abort               abort the operation
  help                show this help

SELECTORS is a comma-separated set such as F1.H1.L1-3,F2.H2,F4-5.
Ranges are allowed only on the final component. Use "all" for every file.
Legend: [ ] none  [~] partial  [x] all
"""


class _ReplSelector:
    def __init__(self, ui: Any, headers: list[Any], operation: str | None) -> None:
        self.ui = ui
        self.index = _PatchIndex(headers)
        self.operation = operation or "record"
        crecord.setappliednodes(headers, False)

    def run(self) -> dict[str, Any]:
        self.ui.write(
            f"Chunk selector REPL ({self.operation}). Selection starts empty.\n"
        )
        _rendernodes(self.ui, self.index, self.index.headers, foldlong=True)
        _writestatus(self.ui, self.index)
        self.ui.write(_HELP)
        while True:
            try:
                command = self.ui._readline("chunk-select>")
            except EOFError:
                raise error.Abort("chunk selection aborted")
            if self.ui.promptecho():
                self.ui.write(command, "\n")
            if self._handle(command.strip()):
                return {}

    def _handle(self, commandline: str) -> bool:
        if not commandline:
            return False
        command, _separator, argument = commandline.partition(" ")
        command = command.lower()
        argument = argument.strip()

        if command == "confirm":
            return self._noargument(command, argument)
        if command == "abort":
            if self._noargument(command, argument):
                raise error.Abort("chunk selection aborted")
            return False
        if command == "help":
            if self._noargument(command, argument):
                self.ui.write(_HELP)
            return False
        if command == "status":
            if self._noargument(command, argument):
                _writestatus(self.ui, self.index)
            return False
        if command == "preview":
            if self._noargument(command, argument):
                _writepreview(self.ui, self.index.headers)
            return False
        if command == "show":
            self._show(argument)
            return False
        if command in {"select", "deselect"}:
            self._setselection(command, argument)
            return False

        self.ui.write(f"ERROR: unknown command: {command}\n")
        return False

    def _noargument(self, command: str, argument: str) -> bool:
        if argument:
            self.ui.write(f"ERROR: {command} does not take an argument\n")
            return False
        return True

    def _resolvetargets(self, argument: str) -> tuple[list[Any], str]:
        if not argument:
            raise SelectorError("expected a selector set or 'all'")
        if argument.lower() == "all":
            return self.index.headers, "all"
        selectors = parseselectors(argument)
        return self.index.resolveall(selectors), ",".join(map(str, selectors))

    def _setselection(self, command: str, argument: str) -> None:
        try:
            nodes, canonical = self._resolvetargets(argument)
        except SelectorError as ex:
            self.ui.write(f"ERROR: {ex}\n")
            return
        applied = command == "select"
        crecord.setappliednodes(self.index.withmarkers(nodes), applied)
        self.ui.write(f"OK: {command}ed {canonical}\n")
        _writestatus(self.ui, self.index)

    def _show(self, argument: str) -> None:
        try:
            nodes, _canonical = self._resolvetargets(argument)
        except SelectorError as ex:
            self.ui.write(f"ERROR: {ex}\n")
            return
        _rendernodes(self.ui, self.index, nodes)


def chunkselector(
    ui: Any, headerlist: list[Any], operation: str | None = None
) -> dict[str, Any]:
    """Run the line-oriented chunk-selector REPL."""
    return _ReplSelector(ui, headerlist, operation).run()
