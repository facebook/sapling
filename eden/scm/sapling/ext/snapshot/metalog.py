# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import time
from typing import Dict, Optional, TypedDict

from bindings import serde
from sapling import perftrace


class SnapshotMetadata(TypedDict, total=False):
    """
    Metadata for a single snapshot/changeset.
    Metadata is cached on snapshot creation.

    Fields:
        bubble: Bubble number for this changeset
        created_at: Unix timestamp when this metadata was created
        bubble_expiration_timestamp: Unix timestamp when the bubble expires (optional)
        # Future fields can be added here
    """

    bubble: int
    created_at: float
    bubble_expiration_timestamp: float


class SnapshotMetadatas(TypedDict, total=False):
    """
    Container for all snapshot metadata.
    This is stored in the metalog as a JSON document and suits as a local cache.
    If metadata is missing or corrupted, we fall back to the remote source of truth.

    This uses a bounded storage approach suitable for metalog:
    - Uses a single static key "snapshot_metadata"
    - Implements LRU eviction to prevent unbounded growth
    - Stores most recent N snapshots (configurable)

    Fields:
        snapshots: Mapping from changeset ID to its metadata
    """

    snapshots: Dict[bytes, SnapshotMetadata]


LATESTSNAPSHOT = "latestsnapshot"
SNAPSHOT_METADATA = "snapshot_metadata"

# Eviction thresholds for snapshot metadata to prevent unbounded growth
# When we reach EVICTION_THRESHOLD, we evict down to EVICTION_TARGET
# This provides hysteresis to avoid frequent evictions
EVICTION_THRESHOLD = 1000  # Start eviction when we have this many entries
EVICTION_TARGET = 100  # Keep this many entries after eviction


@perftrace.tracefunc("Fetch latest snapshot")
def fetchlatestsnapshot(ml):
    return ml.get(LATESTSNAPSHOT)


@perftrace.tracefunc("Snapshot metalog store")
def storelatest(repo, snapshot, bubble, bubble_expiration_timestamp=None) -> None:
    """Store latest snapshot and its bubble metadata in metalog.

    Call this inside repo.transaction() to write changes to disk.

    Args:
        repo: Repository instance
        snapshot: Changeset ID (binary)
        bubble: Bubble ID to store in metadata
        bubble_expiration_timestamp: Optional expiration timestamp for the bubble
    """
    assert repo.currenttransaction()
    ml = repo.metalog()
    if snapshot is not None:
        ml.set(LATESTSNAPSHOT, snapshot)

        # Store bubble metadata for this snapshot
        if bubble is not None:
            metadata = SnapshotMetadata(bubble=bubble, created_at=time.time())
            if bubble_expiration_timestamp is not None:
                metadata["bubble_expiration_timestamp"] = bubble_expiration_timestamp
            storesnapshotmetadata(repo, snapshot, metadata)


# CRUD operations for snapshot metadata


@perftrace.tracefunc("Read snapshot metadatas")
def readmetadatas(ml) -> SnapshotMetadatas:
    """
    Read all snapshot metadatas from metalog.

    Args:
        ml: Metalog instance

    Returns:
        SnapshotMetadatas: Container with all snapshot metadata, empty if not found or corrupted
    """
    data = ml.get(SNAPSHOT_METADATA)
    if data is not None:
        try:
            return serde.cbor_loads(data)
        except Exception:
            # Return empty metadatas if corrupted
            return {}
    return {}


def _evictoldentries(metadatas: SnapshotMetadatas) -> None:
    """
    Evict old entries to keep metadata size bounded using a two-threshold approach.

    This implements a time-based eviction strategy with hysteresis:
    - When we reach EVICTION_THRESHOLD entries, we evict down to EVICTION_TARGET
    - This avoids frequent evictions when entries are added/removed around the threshold
    - Keeps the most recently created entries based on the created_at timestamp

    Args:
        metadatas: Metadatas container to modify in-place
    """
    snapshots = metadatas.get("snapshots", {})
    if len(snapshots) > EVICTION_THRESHOLD:
        # Sort by created_at timestamp, keeping the most recent entries
        sorted_items = sorted(
            snapshots.items(), key=lambda item: item[1].get("created_at", 0)
        )
        # Keep the last EVICTION_TARGET items (most recent)
        keep_items = sorted_items[-EVICTION_TARGET:]
        metadatas["snapshots"] = dict(keep_items)


@perftrace.tracefunc("Store snapshot metadata")
def storesnapshotmetadata(repo, cs_id: bytes, metadata: SnapshotMetadata) -> None:
    """
    Store or update metadata for a specific snapshot/changeset.

    Must be called within a repository transaction.

    Args:
        repo: Repository instance
        cs_id: Changeset ID (binary)
        metadata: Snapshot metadata to store (type-safe)

    Example:
        # Store bubble for a changeset
        metadata = {"bubble": 123}
        storesnapshotmetadata(repo, changeset_id_bytes, metadata)

        # Store multiple fields at once
        metadata = {"bubble": 123, "created_at": time.time()}
        storesnapshotmetadata(repo, changeset_id_bytes, metadata)
    """
    assert repo.currenttransaction(), "Must be called within a transaction"
    ml = repo.metalog()

    # Read existing metadatas
    metadatas = readmetadatas(ml)

    # Initialize snapshots dict if it doesn't exist
    if "snapshots" not in metadatas:
        metadatas["snapshots"] = {}

    # Automatically set created_at timestamp if not provided
    if "created_at" not in metadata:
        metadata = dict(metadata)  # Make a copy to avoid modifying the input
        metadata["created_at"] = time.time()

    # Store the metadata for this changeset ID
    metadatas["snapshots"][cs_id] = metadata

    # Evict old entries to prevent unbounded growth
    _evictoldentries(metadatas)

    # Save back to metalog
    ml.set(SNAPSHOT_METADATA, serde.cbor_dumps(metadatas))


@perftrace.tracefunc("Get snapshot metadata")
def getsnapshotmetadata(ml, cs_id: str) -> Optional[SnapshotMetadata]:
    """
    Get metadata for a specific changeset ID.

    Args:
        ml: Metalog instance
        cs_id: Changeset ID (hex string) to lookup

    Returns:
        Optional[SnapshotMetadata]: Metadata if found, None otherwise
    """
    metadatas = readmetadatas(ml)
    snapshots = metadatas.get("snapshots", {})
    # Convert hex string to bytes for lookup
    cs_id_bytes = bytes.fromhex(cs_id)
    return snapshots.get(cs_id_bytes)


@perftrace.tracefunc("Get changeset ID to bubble mapping")
def getcsidbubblemapping(ml, cs_id: str) -> Optional[int]:
    """
    Get bubble number for a specific changeset ID.

    Args:
        ml: Metalog instance
        cs_id: Changeset ID to lookup

    Returns:
        Optional[int]: Bubble number if found, None otherwise
    """
    metadata = getsnapshotmetadata(ml, cs_id)
    if metadata:
        return metadata.get("bubble")
    return None


@perftrace.tracefunc("Get latest bubble")
def fetchlatestbubble(ml) -> Optional[int]:
    """
    Get bubble number from the latest snapshot.

    The latest bubble is stored in the metalog as the bubble number for the latest snapshot.

    Args:
        ml: Metalog instance

    Returns:
        Optional[int]: Bubble number if found, None otherwise
    """
    latest_snapshot = fetchlatestsnapshot(ml)
    if latest_snapshot is None:
        return None

    # Convert binary changeset ID to hex string
    cs_id_hex = latest_snapshot.hex()
    return getcsidbubblemapping(ml, cs_id_hex)


@perftrace.tracefunc("Delete snapshot metadata")
def deletesnapshotmetadata(repo, cs_id: str) -> bool:
    """
    Delete metadata for a specific changeset.

    Args:
        repo: Repository instance
        cs_id: Changeset ID (hex string) to remove

    Returns:
        bool: True if the metadata was found and deleted, False otherwise
    """
    assert repo.currenttransaction(), "Must be called within a transaction"
    ml = repo.metalog()

    metadatas = readmetadatas(ml)
    snapshots = metadatas.get("snapshots", {})

    # Convert hex string to bytes for lookup
    cs_id_bytes = bytes.fromhex(cs_id)
    if cs_id_bytes in snapshots:
        del snapshots[cs_id_bytes]
        metadatas["snapshots"] = snapshots
        ml.set(SNAPSHOT_METADATA, serde.cbor_dumps(metadatas))
        return True

    return False
