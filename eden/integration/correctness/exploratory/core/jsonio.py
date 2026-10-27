# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Strict JSON decoding, canonical JSON encoding, and the shared validation helpers.

Every core document is read with `loads` and named by `digest`, so two
readers never disagree on what a document is or what it hashes to.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
from collections.abc import Callable
from typing import Any

NAME = re.compile(r"[a-z][a-z0-9-]*")
MAX_ID_BYTES = 128
MAX_SCALAR_BYTES = 4096
MAX_INTEGER = 2**53
_MAX_INTEGER_DIGITS = len(str(MAX_INTEGER))


def require(condition: bool, message: str) -> None:
    """Raise ValueError(message) unless `condition` holds."""
    if not condition:
        raise ValueError(message)


def check_name(value: Any, what: str, max_bytes: int) -> None:
    """Require a lowercase declared name (`NAME`) of at most `max_bytes`."""
    require(
        type(value) is str
        and NAME.fullmatch(value) is not None
        and len(value) <= max_bytes,
        f"bad {what} {value!r:.200}",
    )


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    body: dict[str, Any] = {}
    for key, value in pairs:
        if key in body:
            label = key if len(key) <= 64 else key[:64] + "…"
            raise ValueError(f"duplicate key {label!r}")
        body[key] = value
    return body


def _finite_float(text: str) -> float:
    value = float(text)
    if not math.isfinite(value):
        raise ValueError(f"number {text[:32]} is not finite")
    return value


def _no_constant(name: str) -> Any:
    raise ValueError(f"{name} is not a JSON number")


def _bounded_int(text: str) -> int:
    digits = text.lstrip("-")
    if len(digits) > _MAX_INTEGER_DIGITS:
        raise ValueError(f"integer of {len(digits)} digits is outside ±2**53")
    value = int(text)
    if not -MAX_INTEGER <= value <= MAX_INTEGER:
        raise ValueError(f"integer {text} is outside ±2**53")
    return value


def loads(text: str, *, bounded: bool = False) -> Any:
    """Decode strict JSON: unique keys and finite numbers.

    With `bounded`, integers outside ±`MAX_INTEGER` are refused, by digit
    count before conversion, so no integer text reaches the interpreter's
    conversion limit. Raises ValueError for every malformed document.
    """
    try:
        return json.loads(
            text,
            object_pairs_hook=_unique_object,
            parse_float=_finite_float,
            parse_constant=_no_constant,
            parse_int=_bounded_int if bounded else None,
        )
    except RecursionError as error:
        raise ValueError(f"JSON nests too deeply: {error}") from error


def canonical(value: Any, default: Callable[[Any], Any] | None = None) -> bytes:
    """Return the canonical encoding: sorted keys, compact, raw UTF-8 (surrogates passed)."""
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        default=default,
    ).encode("utf-8", errors="surrogatepass")


def sha256(data: bytes) -> str:
    """Return `sha256:<hex>` of `data`."""
    return "sha256:" + hashlib.sha256(data).hexdigest()


def digest(value: Any) -> str:
    """Return the sha256 name of a JSON value's canonical encoding."""
    return sha256(canonical(value))
