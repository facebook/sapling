# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Source-control channels of the live backend: checkout, status, journal, listing RPCs.

Each function performs one sl process or one Thrift RPC on behalf of a
``LiveBackend`` and returns the scheduler's ``BackendResult``.
"""

from __future__ import annotations

import os
import subprocess
import time
from typing import Any, cast, TYPE_CHECKING

from eden.fs.service.eden.thrift_types import (
    ChangesSinceV2Params,
    CheckoutMode,
    CheckOutRevisionParams,
    DebugInvalidateRequest,
    DirListAttributeDataOrError,
    EdenError,
    GetScmStatusParams,
    JournalPosition,
    MountId,
    PeekCurrentJournalPositionRequest,
    ReaddirParams,
    SyncBehavior,
    TimeSpec,
)
from eden.integration.correctness.exploratory.core.history import FAIL, OK
from eden.integration.correctness.exploratory.core.runspec import Action
from eden.integration.correctness.exploratory.core.scheduler import (
    BackendResult,
    HarnessFailure,
    INVALID_BACKEND_RESPONSE,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    CHECKOUT_FORCE,
    CHECKOUT_NORMAL,
)
from eden.integration.correctness.exploratory.edenfs.encoding import manifest_diff
from eden.integration.correctness.exploratory.edenfs.file_results import status_entries
from eden.integration.correctness.exploratory.edenfs.sapling_processes import (
    kill_wait_for,
)
from eden.thrift.client import EdenNotRunningError
from thrift.python.exceptions import ApplicationError, TransportError

if TYPE_CHECKING:
    from eden.integration.correctness.exploratory.edenfs.backends import LiveBackend

RED_TORN_WRITE = "torn-write"
RED_STATUS_LOSS = "status-loss"
RED_DIFF_LOSS = "diff-loss"
RED_COMMON_STATUS_LOSS = "common-status-loss"
RED_CONTROLS = frozenset(
    {RED_TORN_WRITE, RED_STATUS_LOSS, RED_DIFF_LOSS, RED_COMMON_STATUS_LOSS}
)

# Daemon-side RPC failures the checker judges as FAIL data; anything else
# raised by an RPC is a harness failure. thrift-python raises
# ApplicationError for a server-side failure that is not a declared
# exception (an undeclared C++ exception escaping a handler).
_DAEMON_RPC_FAILURES = (
    EdenNotRunningError,
    TransportError,
    ApplicationError,
    EdenError,
)
_CHECKOUT_MODES = {
    CHECKOUT_NORMAL: CheckoutMode.NORMAL,
    CHECKOUT_FORCE: CheckoutMode.FORCE,
}


def _path_exists(path: str) -> bool:
    """Return whether a mount path exists, treating absence as data."""
    try:
        os.stat(path)
    except (FileNotFoundError, NotADirectoryError):
        return False
    except OSError as error:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE) from error
    return True


def readdir_thrift(backend: LiveBackend, path: str) -> BackendResult:
    """List one directory through one Thrift readdir RPC."""
    params = ReaddirParams(
        mountPoint=os.fsencode(backend._env.mount_path),
        directoryPaths=[os.fsencode(path)],
        requestedAttributes=0,
        sync=SyncBehavior(syncTimeoutSeconds=0),
    )
    try:
        with backend._env.get_thrift_client() as client:
            (listing,) = cast(Any, client).readdir(params).dirLists
    except _DAEMON_RPC_FAILURES as error:
        return BackendResult(FAIL, {"reason": f"rpc: {error}"})
    if listing.get_type() != DirListAttributeDataOrError.Type.dirListAttributeData:
        error = listing.error
        facts: dict[str, Any] = {
            "reason": f"thrift: {error.message}",
            "source": "thrift",
            "error_type": error.errorType.name,
        }
        if error.errorCode is not None:
            facts["errno"] = error.errorCode
        return BackendResult(FAIL, facts)
    names = sorted(os.fsdecode(name) for name in listing.dirListAttributeData)
    return BackendResult(OK, {"channel": "thrift", "entries": names})


def unload_inodes(backend: LiveBackend, action: Action) -> BackendResult:
    """Leave one subtree's files with dentries present but inodes unloaded.

    FUSE lookup loads directly and bypasses the inodemap-load checkpoint;
    only getattr-by-number routes through InodeMap. Mirror the hand-spec
    fixture: stat each file (dentry plus daemon load) and invalidate its
    attrs (dentry kept), then unload the subtree's daemon inodes with one
    `debugInvalidateNonMaterialized` call (unload ignores FUSE refs).
    """
    root = backend._env.mount_path / action.payload["path"]
    mount = os.fsencode(backend._env.mount_path)
    request = DebugInvalidateRequest(
        mount=MountId(mountPoint=mount),
        path=os.fsencode(action.payload["path"]),
        background=False,
        age=TimeSpec(seconds=action.payload.get("age_seconds", 0)),
    )
    try:
        prepared = 0
        with backend._env.get_thrift_client() as client:
            proxy = cast(Any, client)
            for directory, _directories, files in os.walk(root):
                for name in files:
                    full = os.path.join(directory, name)
                    os.stat(full)
                    proxy.invalidateKernelInodeCache(
                        mount,
                        os.fsencode(os.path.relpath(full, backend._env.mount_path)),
                    )
                    prepared += 1
            response = proxy.debugInvalidateNonMaterialized(request)
    except Exception as error:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE) from error
    return BackendResult(
        OK,
        {
            "path": action.payload["path"],
            "prepared": prepared,
            "num_invalidated": response.numInvalidated,
        },
    )


def checkout(backend: LiveBackend, action: Action) -> BackendResult:
    """Run one shared `sl goto`."""
    return backend._executor(action)


def checkout_thrift(backend: LiveBackend, action: Action) -> BackendResult:
    """Move the mount with one blocking checkOutRevision RPC.

    ``mode: force`` sends ``CheckoutMode.FORCE``: the daemon still
    reports every conflict but applies the snapshot anyway. The result
    records each reported conflict's path and `ConflictType` name,
    sorted, beside their count.
    """
    revision = action.payload["revision"]
    node = backend._env.commits[revision].node
    mode = _CHECKOUT_MODES[action.payload["mode"]]
    try:
        with backend._env.get_thrift_client() as client:
            conflicts = cast(Any, client).checkOutRevision(
                os.fsencode(backend._env.mount_path),
                bytes.fromhex(node),
                mode,
                CheckOutRevisionParams(),
            )
        conflict_count = len(conflicts)
        conflict_types = sorted(
            (os.fsdecode(conflict.path), conflict.type.name) for conflict in conflicts
        )
    except Exception as error:
        reason = backend._faults.injected_error_reason(action, error)
        if reason is None and isinstance(error, _DAEMON_RPC_FAILURES):
            reason = str(error)
        if reason is None:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE) from error
        facts: dict[str, Any] = {"reason": reason, "source": "thrift"}
        if isinstance(error, EdenError):
            facts["error_type"] = error.errorType.name
        return BackendResult(FAIL, facts)
    return BackendResult(
        OK,
        {
            "revision": revision,
            "node": node,
            "via": "thrift",
            "conflicts": conflict_count,
            "conflict_types": [
                {"path": path, "type": kind} for path, kind in conflict_types
            ],
        },
    )


def manifest_presence(backend: LiveBackend, action: Action) -> BackendResult:
    """Report FUSE presence for paths changed across two manifests."""
    layout = getattr(backend._env, "_layout", None)
    if layout is None:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    deleted, added, _type_changed = manifest_diff(
        layout, action.payload["from_revision"], action.payload["to_revision"]
    )
    mount = str(backend._env.mount_path)
    return BackendResult(
        OK,
        {
            "deleted_presence": [
                {"path": path, "present": _path_exists(os.path.join(mount, path))}
                for path in sorted(deleted)
            ],
            "added_presence": [
                {"path": path, "present": _path_exists(os.path.join(mount, path))}
                for path in sorted(added)
            ],
        },
    )


def peek_journal_cursor(backend: LiveBackend) -> tuple[int, int]:
    """Return the mount's current journal generation and sequence."""
    request = PeekCurrentJournalPositionRequest(
        mountId=MountId(mountPoint=os.fsencode(backend._env.mount_path))
    )
    with backend._env.get_thrift_client() as client:
        response = cast(Any, client).peekCurrentJournalPosition(request)
    return response.position.mountGeneration, response.position.sequenceNumber


def changed_paths_since(
    backend: LiveBackend, generation: int, since: int
) -> dict[str, Any]:
    """Normalize bounded journal changes and preserve explicit loss reasons."""
    request = ChangesSinceV2Params(
        mountPoint=os.fsencode(backend._env.mount_path),
        fromPosition=JournalPosition(mountGeneration=generation, sequenceNumber=since),
        sync=SyncBehavior(syncTimeoutSeconds=0),
    )
    with backend._env.get_thrift_client() as client:
        response = cast(Any, client).changesSinceV2(request)
    paths: dict[str, dict[str, bool]] = {}
    lost: list[str] = []
    for change in response.changes:
        kind = change.get_type().name
        entry = getattr(change, kind)
        name = entry.get_type().name
        value = getattr(entry, name)
        if kind == "largeChange":
            if name == "lostChanges":
                lost.append(value.reason.name)
            elif name != "commitTransition":
                raise ValueError(f"unsupported journal change: {name}")
            continue
        if kind != "smallChange":
            raise ValueError(f"unsupported journal change: {kind}")
        if name in {"added", "modified", "removed"}:
            entries = [(value.path, name != "added", name != "removed")]
        elif name in {"renamed", "replaced"}:
            entries = [
                (value.from_, True, False),
                (value.to, name == "replaced", True),
            ]
        else:
            raise ValueError(f"unsupported journal change: {name}")
        for path, before, after in entries:
            paths[path.decode("utf-8")] = {
                "existed_before": before,
                "existed_after": after,
            }
    if response.toPosition.mountGeneration != generation and not lost:
        raise ValueError("journal generation changed without a loss notification")
    return {
        "mount_generation": response.toPosition.mountGeneration,
        "sequence_number": response.toPosition.sequenceNumber,
        "changed_paths": [{"path": path, **paths[path]} for path in sorted(paths)],
        "truncated": "JOURNAL_TRUNCATED" in lost,
        "lost_changes": lost,
    }


def journal_window(backend: LiveBackend, action: Action) -> BackendResult:
    """Report the current cursor or a requested changesSinceV2 window."""
    try:
        generation, sequence = peek_journal_cursor(backend)
        facts: dict[str, Any] = {
            "mount_generation": generation,
            "sequence_number": sequence,
        }
        since = action.payload.get("since_sequence")
        if since is not None:
            facts["since_sequence"] = since
            facts.update(changed_paths_since(backend, generation, since))
    except _DAEMON_RPC_FAILURES as error:
        return BackendResult(FAIL, {"reason": str(error), "source": "thrift"})
    except Exception as error:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE) from error
    return BackendResult(OK, facts)


def status_counts(backend: LiveBackend, action: Action) -> BackendResult:
    """Summarize sl status, retaining a refusal as observation data."""
    result = run_sl_process(
        backend, action, ["status", "--print0"], primary=False, started=time.monotonic()
    )
    if result.returncode != 0:
        refusal = (result.stderr or result.stdout).splitlines()[:1]
        return BackendResult(OK, {"refused": refusal[0] if refusal else ""})
    status = {entry[2:]: entry[0] for entry in result.stdout.split("\0") if entry}
    counts: dict[str, int] = {}
    sample: list[str] = []
    for path in sorted(status):
        flag = status[path]
        counts[flag] = counts.get(flag, 0) + 1
        if len(sample) < 20:
            sample.append(f"{flag} {path}")
    return BackendResult(
        OK,
        {
            "counts": [
                {"code": code, "count": counts[code]} for code in sorted(counts)
            ],
            "sample": sample,
        },
    )


def run_sl_process(
    backend: LiveBackend,
    action: Action,
    args: list[str],
    *,
    primary: bool,
    started: float,
) -> subprocess.CompletedProcess[str]:
    """Run one actual sl process of an action under the one Sapling process rule.

    An action's first process is published under the action's ID, so a spec
    ``kill_process`` can target it; a later one (status's diff) under
    ``<action>/<step>``. The leader runs until it exits or containment kills
    it; whatever it leaves running is killed when it exits, within the
    operation's ``kill_wait_for`` budget, as on the reference side.
    """
    mount_repo = backend._env.mount_repo
    return backend._sl.run(
        action.id if primary else f"{action.id}/{args[0]}",
        [mount_repo.hg_bin, "--traceback", *args],
        kill_wait_for(backend._spec.budgets.operation_deadline_seconds),
        cwd=str(mount_repo.path),
        env=mount_repo.hg_environment,
    )


def status_sl(backend: LiveBackend, action: Action) -> BackendResult:
    """Run one shared sl status probe, then apply the red control."""
    result = backend._executor(action)
    if result.outcome != OK:
        return result
    facts: dict[str, Any] = {
        "status": {entry["path"]: entry["code"] for entry in result.facts["status"]},
        "diff_paths": list(result.facts["diff_paths"]),
    }
    _apply_status_control(backend._red_control, facts)
    facts["status"] = status_entries(facts["status"])
    return BackendResult(OK, facts)


def _apply_status_control(red_control: str | None, facts: dict[str, Any]) -> None:
    """Drop the first modified path from status, diff, or both, per the red control.

    ``common-status-loss`` drops it from both: sl status and sl diff share
    one serving path, so they then agree with each other against the model.
    """
    lose_status = red_control in (RED_STATUS_LOSS, RED_COMMON_STATUS_LOSS)
    lose_diff = red_control in (RED_DIFF_LOSS, RED_COMMON_STATUS_LOSS)
    if not (lose_status or lose_diff):
        return
    for path in sorted(facts["status"]):
        if facts["status"][path] == "M":
            if lose_status:
                del facts["status"][path]
            if lose_diff and path in facts["diff_paths"]:
                facts["diff_paths"].remove(path)
            return


def status_thrift(backend: LiveBackend, action: Action) -> BackendResult:
    """Call getScmStatusV2 once for the selected commit.

    An `EdenError` is the action's failure with its error type: whether it
    is legal depends on state (a status of Sapling's `.` fails
    `OUT_OF_DATE_PARENT` while EdenFS's parent has moved past it). A
    malformed response is a harness failure.
    """
    revision = action.payload.get("revision")
    if revision is None:
        parent = run_sl_process(
            backend,
            action,
            ["log", "-r", ".", "-T", "{node}"],
            primary=False,
            started=time.monotonic(),
        )
        if parent.returncode != 0:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        node = parent.stdout.strip()
    else:
        node = backend._env.commits[revision].node
    try:
        commit = bytes.fromhex(node)
        if len(commit) != 20:
            raise ValueError("bad commit length")
        request = GetScmStatusParams(
            mountPoint=os.fsencode(backend._env.mount_path),
            commit=commit,
        )
        with backend._env.get_thrift_client() as client:
            response = cast(Any, client).getScmStatusV2(request)
        entries = response.status.entries
    except EdenError as error:
        return BackendResult(
            FAIL,
            {
                "reason": f"thrift: {error.message}",
                "source": "thrift",
                "error_type": error.errorType.name,
            },
        )
    except Exception as error:  # noqa: BLE001 - invalid RPC response
        raise HarnessFailure(INVALID_BACKEND_RESPONSE) from error
    status = {
        path.decode("utf-8", "replace"): flag.name for path, flag in entries.items()
    }
    return BackendResult(OK, {"status": status_entries(status), "parent": node})
