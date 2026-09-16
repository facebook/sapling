# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json

from .. import simplemerge
from ..i18n import _
from .cmdtable import command

PROTOCOL_VERSION = 1

# Conflict output repeats all three sides in addition to the merged text, so
# cap the whole batch as well as individual fields to bound submit-time memory.
# Keep the record cap below three times the string cap so both are effective.
MAX_STRING_BYTES = 1 << 20
MAX_RECORD_BYTES = 2 << 20
MAX_REQUEST_BYTES = 4 << 20
MAX_RECORDS = 4096

_SIDES = ("base", "local", "remote")


class _ProtocolError(Exception):
    """A malformed request.

    Carries only a typed code and the offending record's caller-supplied id and
    index: never request text, which may hold private commit content.
    """

    def __init__(self, code, index=None, record_id=None, side=None):
        super().__init__(code)
        self.code = code
        self.index = index
        self.record_id = record_id
        self.side = side

    def todict(self):
        obj = {"code": self.code}
        if self.index is not None:
            obj["recordIndex"] = self.index
        if self.record_id is not None:
            obj["recordId"] = self.record_id
        if self.side is not None:
            obj["side"] = self.side
        return obj


def _readupto(fin, limit):
    """Read up to ``limit`` bytes. A pipe read returns one buffer at a time, so
    a single read(limit) would silently truncate a large request."""
    chunks = []
    remaining = limit
    while remaining > 0:
        chunk = fin.read(remaining)
        if not chunk:
            break
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def _readrequest(ui):
    # Stop one byte past the limit so an oversized request is rejected without
    # buffering or parsing it.
    raw = _readupto(ui.fin, MAX_REQUEST_BYTES + 1)
    if len(raw) > MAX_REQUEST_BYTES:
        raise _ProtocolError("REQUEST_TOO_LARGE")
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        raise _ProtocolError("INVALID_UTF8")
    try:
        request = json.loads(text)
    except ValueError:
        raise _ProtocolError("INVALID_JSON")
    if not isinstance(request, dict):
        raise _ProtocolError("INVALID_REQUEST")
    # Exact int, not just == : in Python True == 1 and 1.0 == 1, and this is
    # the one field gating every future semantic change.
    version = request.get("protocolVersion")
    if type(version) is not int or version != PROTOCOL_VERSION:
        raise _ProtocolError("UNSUPPORTED_PROTOCOL_VERSION")
    records = request.get("records")
    if not isinstance(records, list):
        raise _ProtocolError("INVALID_REQUEST")
    if len(records) > MAX_RECORDS:
        raise _ProtocolError("TOO_MANY_RECORDS")
    return records


def _recordid(record, index):
    if "id" not in record:
        raise _ProtocolError("MISSING_FIELD", index)
    record_id = record["id"]
    if not isinstance(record_id, str):
        raise _ProtocolError("INVALID_TYPE", index)
    return record_id


def _encodeside(record, index, record_id, side):
    if side not in record:
        raise _ProtocolError("MISSING_FIELD", index, record_id, side)
    value = record[side]
    if not isinstance(value, str):
        raise _ProtocolError("INVALID_TYPE", index, record_id, side)
    if "\x00" in value:
        raise _ProtocolError("NUL_BYTE", index, record_id, side)
    try:
        # Also rejects lone surrogates, which JSON can carry as \udXXX escapes
        # but which have no valid UTF-8 encoding.
        encoded = value.encode("utf-8")
    except UnicodeEncodeError:
        raise _ProtocolError("LONE_SURROGATE", index, record_id, side)
    if len(encoded) > MAX_STRING_BYTES:
        raise _ProtocolError("STRING_TOO_LARGE", index, record_id, side)
    return encoded


def _parserecords(records):
    parsed = []
    seen = set()
    for index, record in enumerate(records):
        if not isinstance(record, dict):
            raise _ProtocolError("INVALID_RECORD", index)
        record_id = _recordid(record, index)
        if record_id in seen:
            raise _ProtocolError("DUPLICATE_RECORD_ID", index, record_id)
        seen.add(record_id)
        sides = tuple(_encodeside(record, index, record_id, side) for side in _SIDES)
        if sum(len(side) for side in sides) > MAX_RECORD_BYTES:
            raise _ProtocolError("RECORD_TOO_LARGE", index, record_id)
        parsed.append((record_id, sides))
    return parsed


def _mergerecord(base, local, remote):
    """Merge one record's three sides, preserving the caller's newline shape.

    Merge3Text is line based, so a final line without a terminator would never
    match a terminated one. Each non-empty side gets a synthetic trailing LF,
    and the merged result keeps a trailing LF only if `local` or `remote`
    supplied a real one. `base` deliberately does not vote: when both live
    sides dropped a terminator, they agree, and re-adding one from an ancestor
    would return text neither side wrote.
    """
    if local == remote:
        return _recordresult(local)

    padded = [
        text + b"\n" if text and not text.endswith(b"\n") else text
        for text in (base, local, remote)
    ]
    lines = []
    conflicts = []
    for kind, group in simplemerge.Merge3Text(*padded).merge_groups():
        if kind == "conflict":
            base_lines, local_lines, remote_lines = group
            start = len(lines)
            lines.extend(local_lines)
            conflicts.append(
                {
                    "base": b"".join(base_lines).decode("utf-8"),
                    "local": b"".join(local_lines).decode("utf-8"),
                    "remote": b"".join(remote_lines).decode("utf-8"),
                    "mergedStart": start,
                    "mergedEnd": len(lines),
                }
            )
        else:
            lines.extend(group)

    joined = b"".join(lines)
    strip = not (local.endswith(b"\n") or remote.endswith(b"\n"))
    merged = joined[:-1] if strip else joined
    return _recordresult(merged, lines, conflicts, strip)


def _recordresult(merged, lines=(), conflicts=(), strip=False):
    result = {
        "status": "conflict" if conflicts else "clean",
        "merged": merged.decode("utf-8"),
        "conflicts": list(conflicts),
    }
    if conflicts:
        result["mergedLines"] = [line.decode("utf-8") for line in lines]
        result["finalNewlineStripped"] = strip
    return result


def _writejson(ui, obj):
    # Explicit LF, and no ui.write(): the response is a machine payload that
    # must not pick up platform newline translation or output labels.
    payload = json.dumps(obj, ensure_ascii=False, sort_keys=True)
    ui.writebytes(payload.encode("utf-8") + b"\n")


@command(
    "debugmergetext",
    [
        (
            "",
            "protocol-version",
            False,
            _("print the supported protocol version and exit"),
        ),
    ],
    "",
    norepo=True,
)
def debugmergetext(ui, **opts):
    """three-way merge arbitrary text for automation consumption

    Reads JSON on stdin and writes one JSON response on stdout, using
    Sapling's merge algorithm without accessing a repository. The caller
    supplies the common ancestor for each record::

        {
          "protocolVersion": 1,
          "records": [
            {"id": "summary", "base": "...", "local": "...", "remote": "..."}
          ]
        }

    The response preserves record ids and order::

        {
          "protocolVersion": 1,
          "records": [
            {"id": "summary", "status": "clean", "merged": "...", "conflicts": []}
          ]
        }

    Conflicted records use ``status: "conflict"`` and also provide
    ``mergedLines`` and ``finalNewlineStripped``. Each conflict contains::

        {
          "base": "...", "local": "...", "remote": "...",
          "mergedStart": 3, "mergedEnd": 6
        }

    ``merged`` fills conflicted regions with local text. Ranges are zero-based,
    half-open line indices into ``mergedLines``, ascending and non-overlapping;
    a local deletion produces an empty range. To resolve conflicts, replace
    these ranges from last to first, join, and drop one trailing LF if
    ``finalNewlineStripped`` is set. Use these ranges rather than scanning for
    markers, which may occur in the input text.

    Lines split on LF only and keep their terminators; CR is preserved.
    Non-empty inputs are padded with a trailing LF for matching. The result
    keeps it only if local or remote supplied one. Inputs must be valid UTF-8
    without NUL. Changes to this contract require a new protocol version.

    Conflicts exit 0. Invalid requests and merge failures exit 1 with
    ``{"protocolVersion": 1, "error": {"code": ...}}``. Errors contain a code
    and, when applicable, the record's id, index, and side, never input text.

    Pass --protocol-version to probe support without sending a merge request.
    """
    if opts.get("protocol_version"):
        _writejson(ui, {"protocolVersion": PROTOCOL_VERSION})
        return 0

    try:
        parsed = _parserecords(_readrequest(ui))

        records = []
        for index, (record_id, sides) in enumerate(parsed):
            record = {"id": record_id}
            try:
                record.update(_mergerecord(*sides))
            except Exception:
                # Merge internals can raise with input lines embedded in the
                # message; surface a typed code so no commit text escapes.
                raise _ProtocolError("MERGE_FAILED", index, record_id)
            records.append(record)
    except _ProtocolError as err:
        _writejson(ui, {"protocolVersion": PROTOCOL_VERSION, "error": err.todict()})
        return 1

    _writejson(
        ui,
        {
            "protocolVersion": PROTOCOL_VERSION,
            "records": records,
        },
    )
    return 0
