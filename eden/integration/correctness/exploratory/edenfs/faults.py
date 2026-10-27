# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Fault driver for exploratory live runs; the daemon-owning runner runs it.

Install clears stale faults, then injects one fault at an ``edenfs.sites`` checkpoint;
checkout-begin and status-cache-fill park outside every lock peers need. A block is
witnessed when getBlockedFaults (0.01s polls, 10s cap) reports its exact raw key. A
remove with no hit or a leftover block fails the run; unwind force-removes all installs
before daemon stop. A parked checkout holds sl's dry-run pass: overlap, not mutation.
"""

from __future__ import annotations

import os
import re
import time
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, TypeVar

from eden.fs.service.eden.thrift_types import (
    DIS_NOT_RECURSIVE,
    FaultDefinition,
    GetBlockedFaultsRequest,
    RemoveFaultArg,
    SyncBehavior,
    UnblockFaultArg,
)
from eden.integration.correctness.exploratory.core.history import CLEANUP_REMOVED, Event
from eden.integration.correctness.exploratory.core.processes import poll_until
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    RunSpec,
    SpecError,
)
from eden.integration.correctness.exploratory.core.scheduler import (
    BackendResult,
    FAULT_WITNESS_FAILED,
    HarnessFailure,
    INVALID_BACKEND_RESPONSE,
    OK,
)
from eden.integration.correctness.exploratory.edenfs.actions import repository
from eden.integration.correctness.exploratory.edenfs.sites import (
    FETCH_TREE_SITE,
    INODEMAP_LOAD_SITE,
    JOURNAL_RECORD_SITE,
    OVERLAY_WRITE_SITE,
    PENDING_WRITES_SITE,
    SITES,
    TREE_CHECKOUT_SITE,
)
from eden.thrift.client import EdenNotRunningError
from thrift.python.exceptions import TransportError

POLL_SECONDS = 0.01
HIT_TIMEOUT_SECONDS = 10.0
DRAIN_TIMEOUT_SECONDS = 1.0
REMOVE_LIMIT = 8

_T = TypeVar("_T")


def _lookup_name(spec: RunSpec, target: Action) -> str:
    """Return the lookup namepiece a name-keyed install parks.

    A fuse-channel read target opens its path, so the checkpoint fires
    with that register's basename. A checkout target carries no path:
    key by the spec's first register instead, and prefer fuse-channel read
    targets for witnessed parks.
    """
    if target.kind == "read" and target.payload.get("channel") == "fuse":
        path = target.payload.get("path")
        if not path:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        return PurePosixPath(path).name
    if target.kind == "checkout":
        if type(target.payload.get("revision")) is not int:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        paths = repository(spec)["registers"]
        if not paths:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        return PurePosixPath(paths[0]).name
    raise HarnessFailure(INVALID_BACKEND_RESPONSE)


@dataclass
class _Installed:
    """One injected fault plus its witness/removal bookkeeping."""

    fault_id: str
    key_class: str
    selector: str
    witness: str | None
    behavior: str
    target_action: str
    hit: bool = False
    consumed: bool | None = None
    target_completion: BackendResult | None = None
    # The one-shot registration itself succeeded; until then nothing can witness it.
    registered: bool = False


def _resolve_fetch_tree(
    spec: RunSpec, action: Action, target: Action, env: Any, key_class: str
) -> _Installed:
    """Resolve a root-tree install to its exact request witness."""
    revision = target.payload.get("revision")
    channel = target.payload.get("channel")
    site = SITES[FETCH_TREE_SITE]
    if action.payload.get("behavior") not in site.behaviors:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    if action.payload.get("ordinal", 1) != 1:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    if spec.environment.target.get("fixture_source") != "backing":
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    if channel not in {"sl", "thrift"}:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    commit_count = spec.environment.target.get("repository", {}).get("commit_count")
    if type(commit_count) is not int or commit_count != len(env.commits):
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    if revision is not None and (
        type(revision) is not int or not 0 <= revision < commit_count
    ):
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    if channel == "thrift" and revision not in (None, commit_count - 1):
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    try:
        if channel == "sl" or revision is None:
            witness = env.mount_repo.hg("log", "-r", ".", "-T", "{node}").strip()
        else:
            witness = env.commits[revision].node
        if len(bytes.fromhex(witness)) != 20:
            raise ValueError("bad commit length")
    except (AttributeError, IndexError, TypeError, ValueError) as error:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE) from error
    return _Installed(
        action.payload["fault_id"],
        key_class,
        ".*",
        witness,
        action.payload["behavior"],
        target.id,
    )


def _resolve_tree_checkout(
    action: Action, target: Action, key_class: str
) -> _Installed:
    """Resolve an ordinal checkout install over one directory subtree."""
    key = action.payload.get("key")
    ordinal = action.payload.get("ordinal", 1)
    if (
        target.payload.get("via") != "thrift"
        or type(key) is not str
        or not key
        or type(ordinal) is not int
        or ordinal < 1
    ):
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    selector = re.escape(key) + r"(/.*)?, .*"
    return _Installed(
        action.payload["fault_id"],
        key_class,
        selector,
        None,
        action.payload["behavior"],
        target.id,
    )


def _target_for_install(
    spec: RunSpec, action: Action, target_kinds: frozenset[str], site_name: str
) -> Action:
    """Resolve and validate the operation named by a fault install."""
    try:
        target = spec.action(action.payload.get("target_action", ""))
    except SpecError as error:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE) from error
    if target.kind not in target_kinds:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    if target.kind == "checkout" and type(target.payload.get("revision")) is not int:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    if site_name == PENDING_WRITES_SITE and target.payload.get(
        "sync_timeout_seconds"
    ) in (None, 0):
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    return target


def _resolve_file_checkpoint(action: Action, target: Action, env: Any) -> _Installed:
    """Bind a file checkpoint without loading an unloaded target inode."""
    site_name = action.payload["site"]
    path = PurePosixPath(target.payload["path"])
    if site_name == JOURNAL_RECORD_SITE:
        witness = str(path)
    elif site_name == OVERLAY_WRITE_SITE:
        witness = str((Path(env.mount_path) / path).stat().st_ino)
    else:
        with env.get_thrift_client() as client:
            trees = client.debugInodeStatus(
                os.fsencode(env.mount_path),
                os.fsencode(path.parent),
                DIS_NOT_RECURSIVE,
                SyncBehavior(syncTimeoutSeconds=0),
            )
        inode = next(
            (
                entry.inodeNumber
                for tree in trees
                for entry in tree.entries
                if os.fsdecode(entry.name) == path.name
            ),
            0,
        )
        if inode <= 0:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        witness = str(inode)
    return _Installed(
        action.payload["fault_id"],
        SITES[site_name].key_class,
        re.escape(witness),
        witness,
        action.payload["behavior"],
        target.id,
    )


def _resolve(spec: RunSpec, action: Action, env: Any) -> _Installed:
    """Resolve an install to its key class, selector, and witness value.

    The target must be a kind the site can reach (``Site.target_kinds``; the
    pending-writes site needs a nonzero-sync read). Checkout-begin keys the mount
    path, so every checkout on the mount hits; fixed-key sites key the checkpoint
    literal; name-keyed sites key the target's basename. Each such selector is the
    regex escape of its raw witness. Other sites delegate to their own resolvers.
    """
    fault_id = action.payload["fault_id"]
    site_name = action.payload.get("site", "")
    site = SITES.get(site_name)
    if site is None:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    target = _target_for_install(spec, action, site.target_kinds, site_name)
    if site_name in {OVERLAY_WRITE_SITE, INODEMAP_LOAD_SITE, JOURNAL_RECORD_SITE}:
        return _resolve_file_checkpoint(action, target, env)
    if site_name == FETCH_TREE_SITE:
        return _resolve_fetch_tree(spec, action, target, env, site.key_class)
    if site_name == TREE_CHECKOUT_SITE:
        return _resolve_tree_checkout(action, target, site.key_class)
    if site.fixed_key is not None:
        return _Installed(
            fault_id,
            site.key_class,
            re.escape(site.fixed_key),
            site.fixed_key,
            action.payload["behavior"],
            target.id,
        )
    if site.name_keyed:
        name = _lookup_name(spec, target)
        return _Installed(
            fault_id,
            site.key_class,
            re.escape(name),
            name,
            action.payload["behavior"],
            target.id,
        )
    try:
        mount_path = str(env.mount_path)
    except RuntimeError as error:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE) from error
    if not mount_path:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    return _Installed(
        fault_id,
        site.key_class,
        re.escape(mount_path),
        mount_path,
        action.payload["behavior"],
        target.id,
    )


def _daemon_alive(env: Any) -> bool:
    """Return whether the daemon's recorded pid still exists."""
    pid = env.daemon_pid
    if pid is None:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


class _CleanupRetry:
    """Cleanup Thrift calls, each on its own connection and retried once.

    A call that raises EdenNotRunningError or TransportError is retried
    only while the daemon pid is alive: the client reports a refused or
    reset connection to a live daemon the same way as a stopped one.
    """

    def __init__(self, env: Any) -> None:
        self._env = env
        self.count = 0

    def call(self, rpc: Callable[[Any], _T]) -> _T:
        try:
            with self._env.get_thrift_client() as client:
                return rpc(client)
        except (EdenNotRunningError, TransportError):
            if not _daemon_alive(self._env):
                raise
            self.count += 1
        with self._env.get_thrift_client() as client:
            return rpc(client)


@contextmanager
def _session(
    env: Any, retry: _CleanupRetry | None
) -> Iterator[Callable[[Callable[[Any], Any]], Any]]:
    """Yield a runner for Thrift calls: one shared client, or ``retry``'s calls."""
    if retry is not None:
        yield retry.call
        return
    with env.get_thrift_client() as client:
        yield lambda rpc: rpc(client)


def _blocked(env: Any, key_class: str, retry: _CleanupRetry | None = None) -> list[str]:
    """Return the daemon's currently blocked key values for one key class."""
    request = GetBlockedFaultsRequest(keyclass=key_class)
    with _session(env, retry) as call:
        return list(call(lambda client: client.getBlockedFaults(request).keyValues))


def _clear(
    env: Any, key_class: str, selector: str, retry: _CleanupRetry | None = None
) -> int:
    """Remove a fault idempotently, then unblock; return removal count."""
    removed = 0
    remove = RemoveFaultArg(keyClass=key_class, keyValueRegex=selector)
    unblock = UnblockFaultArg(keyClass=key_class, keyValueRegex=selector)
    with _session(env, retry) as call:
        for _ in range(REMOVE_LIMIT):
            if not call(lambda client: client.removeFault(remove)):
                break
            removed += 1
        call(lambda client: client.unblockFault(unblock))
    if removed == REMOVE_LIMIT:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    return removed


def _drain(env: Any, key_class: str, retry: _CleanupRetry | None = None) -> list[str]:
    """Poll until no key stays blocked; return the keys still blocked at the deadline."""
    blocked: list[str] = []

    def drained() -> bool:
        blocked[:] = _blocked(env, key_class, retry)
        return not blocked

    poll_until(drained, time.monotonic() + DRAIN_TIMEOUT_SECONDS, POLL_SECONDS)
    return blocked


class FaultTracker:
    """Armed faults for one live run, with completion and drain accounting."""

    def __init__(self, env: Any, spec: RunSpec) -> None:
        self._env = env
        self._spec = spec
        self._installed: dict[str, _Installed] = {}

    def install(self, action: Action) -> BackendResult:
        """Clear stale faults, then inject the requested one-shot behavior.

        The install is tracked before any registration, so `unwind` removes
        whatever a raising install left behind (an ordinal prefix included);
        it can count as witnessed only once its one-shot registration succeeded.
        """
        installed = _resolve(self._spec, action, self._env)
        fault_id = action.payload["fault_id"]
        _clear(self._env, installed.key_class, installed.selector)
        behavior = installed.behavior
        ordinal = action.payload.get("ordinal", 1)
        if type(ordinal) is not int or ordinal < 1:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        if behavior not in ("block", "error", "delay"):
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        self._installed[fault_id] = installed
        if ordinal > 1:
            with self._env.get_thrift_client() as client:
                client.injectFault(
                    FaultDefinition(
                        keyClass=installed.key_class,
                        keyValueRegex=installed.selector,
                        count=ordinal - 1,
                    )
                )
        definition: dict[str, Any] = {
            "keyClass": installed.key_class,
            "keyValueRegex": installed.selector,
            "count": 1,
        }
        if behavior == "block":
            definition["block"] = True
        elif behavior == "error":
            definition.update(
                errorType="runtime_error",
                errorMessage=f"exploratory fault {fault_id}",
            )
        else:
            definition["delayMilliseconds"] = action.payload["delay_ms"]
        with self._env.get_thrift_client() as client:
            client.injectFault(FaultDefinition(**definition))
        installed.registered = True
        return BackendResult(
            OK,
            {
                "fault_id": fault_id,
                "selector": installed.selector,
                "ordinal": ordinal,
            },
        )

    def record_completion(self, action: Action, result: BackendResult) -> BackendResult:
        """Record and return a target completion for non-block fault witnesses."""
        for installed in tuple(self._installed.values()):
            if installed.behavior == "block" or installed.target_action != action.id:
                continue
            installed.target_completion = result
        return result

    def injected_error_reason(self, action: Action, error: Exception) -> str | None:
        """Return a reason only when an installed error fault caused the error."""
        for installed in tuple(self._installed.values()):
            message = rf"\bexploratory fault {re.escape(installed.fault_id)}\b"
            if (
                installed.behavior == "error"
                and installed.target_action == action.id
                and re.search(message, str(error))
            ):
                return f"checkout: {error}"
        return None

    def wait_for_hit(self, action: Action) -> BackendResult:
        """Poll for the exact witness value; anything else is witness-failed.

        The daemon reports raw values. An install with a resolved witness
        is hit only when that exact value is blocked: no substring false
        positives across sibling mounts. A tree-checkout install has no
        witness, so the first blocked value its selector fully matches
        becomes the witness.
        """
        installed = self._installed.get(action.payload.get("fault_id", ""))
        if installed is None:
            raise HarnessFailure(FAULT_WITNESS_FAILED)
        if installed.behavior != "block":
            completion = installed.target_completion
            if completion is None:
                raise HarnessFailure(FAULT_WITNESS_FAILED)
            return BackendResult(
                OK,
                {
                    "fault_id": installed.fault_id,
                    "target_completed": True,
                    "target_outcome": completion.outcome,
                },
            )

        def hit() -> list[str]:
            blocked = _blocked(self._env, installed.key_class)
            witness = installed.witness
            if witness is None:
                witness = next(
                    (
                        value
                        for value in blocked
                        if re.fullmatch(installed.selector, value) is not None
                    ),
                    None,
                )
            return [witness] if witness is not None and witness in blocked else []

        found = poll_until(hit, time.monotonic() + HIT_TIMEOUT_SECONDS, POLL_SECONDS)
        if not found:
            raise HarnessFailure(FAULT_WITNESS_FAILED)
        (witness,) = found
        installed.witness = witness
        installed.hit = True
        return BackendResult(
            OK,
            {
                "fault_id": installed.fault_id,
                "hit": True,
                "witness": witness,
            },
        )

    def remove(self, action: Action) -> BackendResult:
        """Remove idempotently, unblock, and verify nothing stays blocked.

        A remove that finds no hit, or a drain that finds a leftover
        registration, is a witness failure: without the parked interval
        the composition proves nothing, so the run cannot pass.
        """
        installed = self._installed.get(action.payload.get("fault_id", ""))
        if installed is None:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        if installed.behavior == "block" and not installed.hit:
            raise HarnessFailure(FAULT_WITNESS_FAILED)
        if installed.behavior != "block" and installed.target_completion is None:
            raise HarnessFailure(FAULT_WITNESS_FAILED)
        removed = _clear(self._env, installed.key_class, installed.selector)
        if installed.behavior != "block":
            installed.consumed = removed == 0
            if not installed.consumed:
                raise HarnessFailure(FAULT_WITNESS_FAILED)
        if _drain(self._env, installed.key_class):
            raise HarnessFailure(FAULT_WITNESS_FAILED)
        return BackendResult(OK, {"fault_id": installed.fault_id, "removed": removed})

    def unwind(self, sink: Any, clock: Any) -> dict[str, int]:
        """Force-remove every install, never raises; record cleanup-removed each."""

        summary = {"unwound": 0, "misses": 0, "leaks": 0}
        for installed in self._installed.values():
            facts: dict[str, Any] = {
                "fault_id": installed.fault_id,
                "leak": False,
                "blocked_keys": None,
            }
            retry = _CleanupRetry(self._env)
            try:
                removed = _clear(
                    self._env, installed.key_class, installed.selector, retry
                )
                facts["removed"] = removed
                if (
                    installed.behavior != "block"
                    and installed.consumed is None
                    and installed.registered
                ):
                    installed.consumed = removed == 0
                facts["blocked_keys"] = _drain(self._env, installed.key_class, retry)
                facts["leak"] = bool(facts["blocked_keys"])
            except Exception as error:  # noqa: BLE001 - recorded in facts
                facts["leak"] = True
                facts["error"] = f"{type(error).__name__}: {error}"
            facts["cleanup_retries"] = retry.count
            facts["hit"] = installed.hit
            facts["target_completed"] = installed.target_completion is not None
            facts["consumed"] = installed.consumed
            facts["miss"] = not installed.registered or (
                not installed.hit
                if installed.behavior == "block"
                else installed.consumed is not True
            )
            summary["unwound"] += 1
            summary["misses"] += bool(facts["miss"])
            summary["leaks"] += bool(facts["leak"])
            sink.emit(Event(type=CLEANUP_REMOVED, time=clock(), data=facts))
        return summary
