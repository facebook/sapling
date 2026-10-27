# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Register value encoding and sparse generated-manifest diffs shared by EdenFS modules.

Every planned register image is exactly `REGISTER_SIZE` ASCII bytes. A
write's image names its writer and value ID; a committed register image
names its revision and a fixed-size hash of its path. Writers `init` and
`commit*` are reserved for fixture images, so no write can produce bytes
that equal a committed image.
"""

from __future__ import annotations

import hashlib
from typing import Any

REGISTER_SIZE = 64
INIT_WRITER = "init"
COMMIT_WRITER_PREFIX = "commit"
SCATTER_PAYLOAD = b"q" * REGISTER_SIZE
_PATH_HASH_HEX = 16


def is_reserved_writer(name: str) -> bool:
    """Return whether a writer or value ID lies in the fixture image namespace."""
    return name == INIT_WRITER or name.startswith(COMMIT_WRITER_PREFIX)


def _pad(text: str) -> bytes:
    encoded = text.encode("ascii")
    if len(encoded) > REGISTER_SIZE:
        raise ValueError(f"encoded register value too long: {text!r}")
    return encoded + b"." * (REGISTER_SIZE - len(encoded))


def encode_value(seed: int, writer: str, value_id: str) -> bytes:
    """Return the exact 64 ASCII bytes for one planned register value.

    Shared contract between the live backend, which writes these bytes,
    and effect contracts, which expects them. The fixture's revision-stable
    images use writer `init` and the path as the value ID. A writer or value
    ID containing whitespace or `=` could make two values' fields read the
    same, so it is refused.
    """
    for name in (writer, value_id):
        if not name or any(char.isspace() or char == "=" for char in name):
            raise ValueError(f"register value field {name!r} is empty or ambiguous")
    return _pad(f"REG1 seed={seed:08d} writer={writer} action={value_id}")


def initial_content(seed: int, path: str) -> bytes:
    """Return the revision-stable image of a reserved non-register path."""
    return encode_value(seed, INIT_WRITER, path)


def encode_committed(seed: int, path: str, revision: int) -> bytes:
    """Return the exact 64 bytes a fixture commit holds for one register.

    The image is unique per `(path, revision)`: it names the revision in
    the reserved `commit<revision>` writer and a 16-hex-digit SHA-256 prefix
    of the path, so its size never depends on the path's length.
    """
    if type(revision) is not int or revision < 0:
        raise ValueError(f"bad committed revision {revision!r}")
    identity = hashlib.sha256(path.encode("utf-8")).hexdigest()[:_PATH_HASH_HEX]
    return _pad(
        f"REG1 seed={seed:08d} writer={COMMIT_WRITER_PREFIX}{revision} "
        f"action={identity}"
    )


def content_token(data: bytes) -> str:
    """Return the content token of whole file bytes: their lowercase SHA-256 hex."""
    return hashlib.sha256(data).hexdigest()


def manifest_diff(
    layout: Any, from_revision: int, to_revision: int
) -> tuple[set[str], set[str], set[str]]:
    """Return generated paths deleted, added, and type-changed between two revisions.

    Only indexes whose path changes when entering a revision between the
    two can differ, so the diff costs O(changed indexes), never the whole
    layout. A type-changed path stays present (as a directory or a file);
    only the generated child inside it is added or deleted. Reserved
    register and churn paths are outside the generated layout.
    """
    low, high = sorted((from_revision, to_revision))
    indices = {
        index
        for revision in range(low + 1, high + 1)
        for index in layout.changed_indices(revision)
    }
    old = {layout.file_at(index, from_revision).path for index in indices}
    new = {layout.file_at(index, to_revision).path for index in indices}
    type_changed: set[str] = set()
    for change in layout.type_changes:
        to_directory = layout.is_directory(change.path, to_revision)
        if layout.is_directory(change.path, from_revision) == to_directory:
            continue
        type_changed.add(change.path)
        (new if to_directory else old).add(layout.type_change_child(change.path).path)
    return old - new, new - old, type_changed
