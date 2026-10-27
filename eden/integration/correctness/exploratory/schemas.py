# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Exact versions, keys, and numeric bounds for the run admission and report documents.

Every reader accepts only its exact current schema version and key set; a
missing, boolean, older, newer, or unknown version, an unknown key, a
repeated key, or a non-finite number is a `SchemaError` naming its cause.
Nothing is ever truncated to fit a bound.
"""

from __future__ import annotations

import hashlib
import json
import os
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from enum import StrEnum
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory.core.checker import (
    decode_verdict,
    Verdict,
    VERDICT_OPERATIONAL_BYTES,
)

ADMISSION_SCHEMA_VERSION = 1
FINDINGS_SCHEMA_VERSION = 1

ADMISSION_NAME = "admission.json"
DIAGNOSTICS = "diagnostics"


@dataclass(frozen=True)
class Bounds:
    """The numeric bounds the report path enforces, in one place."""

    verdict_operational_bytes: int = VERDICT_OPERATIONAL_BYTES
    verdict_read_ceiling_bytes: int = 256 * 1024 * 1024
    text_bytes: int = 4096


BOUNDS = Bounds()


class RunPurpose(StrEnum):
    """Why a run happened."""

    EXPLORE = "explore"
    RED_CONTROL = "red-control"
    CALIBRATION = "calibration"
    PINNED_SUT = "pinned-sut"
    REPLAY = "replay"
    MANUAL = "manual"


class SchemaError(ValueError):
    """A document that cannot be read; `decision` names the cause."""

    def __init__(self, decision: str, message: str) -> None:
        super().__init__(f"{decision}: {message}")
        self.decision = decision


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    keys = [key for key, _value in pairs]
    if len(set(keys)) != len(keys):
        raise ValueError(f"repeated JSON key in {sorted(keys)}")
    return dict(pairs)


def _no_constant(name: str) -> Any:
    raise ValueError(f"non-finite JSON number {name}")


def _finite_float(text: str) -> float:
    value = float(text)
    if value in (float("inf"), float("-inf")):
        raise ValueError(f"non-finite JSON number {text}")
    return value


def strict_loads(text: str, decision: str = "schema:malformed") -> Any:
    """Parse JSON, rejecting repeated keys and non-finite numbers."""
    try:
        return json.loads(
            text,
            object_pairs_hook=_unique_object,
            parse_constant=_no_constant,
            parse_float=_finite_float,
        )
    except ValueError as error:
        raise SchemaError(decision, str(error)) from error


def require_text(
    value: Any,
    what: str,
    limit_bytes: int = BOUNDS.text_bytes,
    *,
    decision: str = "schema:malformed",
) -> str:
    """Return a string of at most `limit_bytes` UTF-8 bytes."""
    if type(value) is not str:
        raise SchemaError(decision, f"{what} must be a string, got {value!r}")
    if len(value.encode("utf-8", errors="surrogatepass")) > limit_bytes:
        raise SchemaError(decision, f"{what} exceeds {limit_bytes} UTF-8 bytes")
    return value


def optional_text(
    value: Any, what: str, *, decision: str = "schema:malformed"
) -> str | None:
    """Return None or a bounded string."""
    return None if value is None else require_text(value, what, decision=decision)


def require_bool(value: Any, what: str, *, decision: str = "schema:malformed") -> bool:
    """Return a JSON boolean."""
    if type(value) is not bool:
        raise SchemaError(decision, f"{what} must be a boolean, got {value!r}")
    return value


def require_object(
    raw: Any,
    keys: Iterable[str],
    what: str,
    *,
    decision: str = "schema:malformed",
) -> dict[str, Any]:
    """Return a dict with exactly `keys`."""
    expected = frozenset(keys)
    if type(raw) is not dict or set(raw) != expected:
        found = sorted(raw) if type(raw) is dict else type(raw).__name__
        raise SchemaError(
            decision, f"{what} must have exactly {sorted(expected)}, got {found}"
        )
    return raw


def require_document(
    raw: Any,
    *,
    version: int,
    required: Iterable[str],
    what: str,
    decision: str,
    optional: frozenset[str] = frozenset({DIAGNOSTICS}),
) -> dict[str, Any]:
    """Return a versioned document with exactly its required keys.

    Keys in `optional` may be absent; any other key, or a `schema_version`
    that is not exactly `version` as a JSON integer, raises.
    """
    if type(raw) is not dict:
        raise SchemaError(decision, f"{what} must be an object")
    found = raw.get("schema_version")
    if type(found) is not int or found != version:
        raise SchemaError(
            f"{decision}-version",
            f"{what} schema_version must be {version}, got {found!r}",
        )
    keys = {"schema_version", *required}
    missing = keys - set(raw)
    unknown = set(raw) - keys - optional
    if missing or unknown:
        raise SchemaError(
            decision,
            f"{what} keys: missing {sorted(missing)}, unknown {sorted(unknown)}",
        )
    diagnostics = raw.get(DIAGNOSTICS, {})
    if DIAGNOSTICS in optional and type(diagnostics) is not dict:
        raise SchemaError(decision, f"{what} diagnostics must be an object")
    return raw


def canonical_json(value: Any) -> bytes:
    """Return the canonical UTF-8 JSON bytes used for every digest."""
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    ).encode("utf-8")


def sha256_ref(data: bytes) -> str:
    """Return `sha256:<hex>` of some bytes."""
    return "sha256:" + hashlib.sha256(data).hexdigest()


def document_digest(value: Any) -> str:
    """Return the digest of a document's canonical JSON."""
    return sha256_ref(canonical_json(value))


def write_document(path: Path, document: Mapping[str, Any]) -> None:
    """Write one local JSON document deterministically and durably.

    The text goes to a temporary file in the same directory, is fsynced, and
    replaces `path` atomically, so a reader sees the old document or the new
    one, never a torn write.
    """
    text = json.dumps(document, sort_keys=True, indent=2, allow_nan=False) + "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    with open(temporary, "w", encoding="utf-8") as handle:
        handle.write(text)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


@dataclass(frozen=True)
class VerdictFile:
    """A whole `verdict.json`: the parsed verdict, its exact encoded size, and digest.

    `oversize` marks a verdict past the 2 MiB operational threshold: it is
    still parsed and kept whole, never truncated, and reported as
    `execution.verdict-oversize` with `encoded_bytes` and `sha256` of the
    retained file.
    """

    verdict: Verdict
    encoded_bytes: int
    oversize: bool
    sha256: str = ""


VERDICT_MISSING = "execution.missing-verdict"
VERDICT_UNREADABLE = "execution.verdict-unreadable"
VERDICT_OVERSIZE = "execution.verdict-oversize"


def load_verdict_file(path: Path, bounds: Bounds = BOUNDS) -> VerdictFile:
    """Read a whole `verdict.json` through the checker's exact v1 decoder.

    A missing file raises `execution.missing-verdict`; a file past the read
    ceiling, or one the decoder rejects, raises `execution.verdict-unreadable`.
    """
    try:
        size = os.stat(path).st_size
    except FileNotFoundError as error:
        raise SchemaError(VERDICT_MISSING, str(path)) from error
    except OSError as error:
        raise SchemaError(VERDICT_UNREADABLE, f"{path}: {error}") from error
    if size > bounds.verdict_read_ceiling_bytes:
        raise SchemaError(
            VERDICT_UNREADABLE,
            f"{path}: {size} bytes exceeds the {bounds.verdict_read_ceiling_bytes}-byte read ceiling",
        )
    try:
        data = path.read_bytes()
        verdict = decode_verdict(data)
    except (OSError, ValueError) as error:
        raise SchemaError(VERDICT_UNREADABLE, f"{path}: {error}") from error
    return VerdictFile(
        verdict,
        len(data),
        len(data) > bounds.verdict_operational_bytes,
        "sha256:" + hashlib.sha256(data).hexdigest(),
    )


@dataclass(frozen=True)
class DaemonProvenance:
    """The daemon a run exercised, by reference; executable bytes are never copied."""

    binary_sha256: str | None = None
    source_commit: str | None = None
    source_timestamp: float | None = None
    fatal_eden_bug: bool | None = None
    binary_ref: str | None = None
    log: str | None = None


@dataclass(frozen=True)
class Admission:
    """The runner-owned `admission.json` written beside a run's other artifacts."""

    run_purpose: RunPurpose
    campaign_id: str | None
    environment_degraded: str | None = None
    daemon: DaemonProvenance | None = None
    diagnostics: Mapping[str, Any] = field(default_factory=dict)


_ADMISSION_KEYS = (
    "run_purpose",
    "campaign_id",
    "environment_degraded",
    "daemon",
)
_DAEMON_KEYS = tuple(DaemonProvenance.__dataclass_fields__)
ADMISSION_DECISION = "purpose:invalid-admission"
ADMISSION_MISSING = "purpose:missing-admission"


def admission_for(
    purpose: RunPurpose,
    campaign_id: str | None,
    *,
    environment_degraded: str | None = None,
    daemon: DaemonProvenance | None = None,
) -> Admission:
    """Return the admission a runner writes for one run of this purpose."""
    return Admission(
        run_purpose=purpose,
        campaign_id=campaign_id,
        environment_degraded=environment_degraded,
        daemon=daemon,
    )


def admission_document(admission: Admission) -> dict[str, Any]:
    """Return the exact `admission.json` v1 document."""
    return {
        "schema_version": ADMISSION_SCHEMA_VERSION,
        "run_purpose": str(admission.run_purpose),
        "campaign_id": admission.campaign_id,
        "environment_degraded": admission.environment_degraded,
        "daemon": None
        if admission.daemon is None
        else {key: getattr(admission.daemon, key) for key in _DAEMON_KEYS},
        DIAGNOSTICS: dict(admission.diagnostics),
    }


def write_admission(run_dir: Path, admission: Admission) -> None:
    """Write `admission.json` beside a run; nothing else in the run dir changes."""
    write_document(run_dir / ADMISSION_NAME, admission_document(admission))


def _optional_bool(value: Any, what: str) -> bool | None:
    return (
        None
        if value is None
        else require_bool(value, what, decision=ADMISSION_DECISION)
    )


def _read_daemon(raw: Any) -> DaemonProvenance | None:
    if raw is None:
        return None
    body = require_object(
        raw, _DAEMON_KEYS, "admission daemon", decision=ADMISSION_DECISION
    )
    timestamp = body["source_timestamp"]
    if timestamp is not None and (type(timestamp) not in (int, float)):
        raise SchemaError(ADMISSION_DECISION, f"bad source_timestamp {timestamp!r}")
    text = {
        key: optional_text(body[key], key, decision=ADMISSION_DECISION)
        for key in (
            "binary_sha256",
            "source_commit",
            "binary_ref",
            "log",
        )
    }
    return DaemonProvenance(
        source_timestamp=None if timestamp is None else float(timestamp),
        fatal_eden_bug=_optional_bool(body["fatal_eden_bug"], "fatal_eden_bug"),
        **text,
    )


def read_admission(raw: Any) -> Admission:
    """Parse an exact `admission.json` v1 document."""
    body = require_document(
        raw,
        version=ADMISSION_SCHEMA_VERSION,
        required=_ADMISSION_KEYS,
        what="admission",
        decision=ADMISSION_DECISION,
    )
    raw_purpose = body["run_purpose"]
    if type(raw_purpose) is not str:
        raise SchemaError(
            ADMISSION_DECISION, f"run_purpose {raw_purpose!r} is not text"
        )
    try:
        purpose = RunPurpose(raw_purpose)
    except ValueError as error:
        raise SchemaError(ADMISSION_DECISION, str(error)) from error
    return Admission(
        run_purpose=purpose,
        campaign_id=optional_text(
            body["campaign_id"], "campaign_id", decision=ADMISSION_DECISION
        ),
        environment_degraded=optional_text(
            body["environment_degraded"],
            "environment_degraded",
            decision=ADMISSION_DECISION,
        ),
        daemon=_read_daemon(body["daemon"]),
        diagnostics=_diagnostics(body.get(DIAGNOSTICS, {})),
    )


def _diagnostics(raw: Any) -> dict[str, Any]:
    if type(raw) is not dict:
        raise SchemaError(ADMISSION_DECISION, "diagnostics must be an object")
    return dict(raw)


def load_admission(run_dir: Path) -> Admission:
    """Read a run's `admission.json`; a missing file is `purpose:missing-admission`."""
    path = run_dir / ADMISSION_NAME
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError as error:
        raise SchemaError(ADMISSION_MISSING, str(path)) from error
    except (OSError, UnicodeDecodeError) as error:
        raise SchemaError(ADMISSION_DECISION, f"{path}: {error}") from error
    return read_admission(strict_loads(text, ADMISSION_DECISION))
