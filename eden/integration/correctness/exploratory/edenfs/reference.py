# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The differential reference: a plain-filesystem Sapling checkout of the fixture.

A differential run (``setup_profile.reference_clone``) clones the generated
backing repository with plain Sapling, outside any EdenFS mount, at the
mount's commit, and replays the mount's modeled starting dirt onto it.
``ReferenceCheckout`` owns that directory and every reference process;
``ReferenceBackend`` runs one shared action on it. Reference processes live
in their own process registry under ``reference/<action>/<step>`` keys, so
they never collide with EdenFS action IDs or the containment that kills
EdenFS process groups. A reference process that leaves anything running
when its leader exits fails, and a reference side without a trustworthy observation
raises ``ReferenceFailure``: the runtime never invents an outcome.
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import threading
import time
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory.core.differential_schedule import (
    Applicability,
    not_applicable,
    SHARED,
)
from eden.integration.correctness.exploratory.core.history import OK
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.core.scheduler import BackendResult
from eden.integration.correctness.exploratory.core.target import ProcessRegistry
from eden.integration.correctness.exploratory.edenfs.actions import (
    CASE_INSENSITIVE_MOUNT,
    reference_clone,
    ReferenceNotApplicable,
)
from eden.integration.correctness.exploratory.edenfs.encoding import encode_value
from eden.integration.correctness.exploratory.edenfs.executor import (
    serves,
    SharedExecutor,
)
from eden.integration.correctness.exploratory.edenfs.file_results import (
    parse_status,
    write_image,
)
from eden.integration.correctness.exploratory.edenfs.sapling_processes import (
    kill_wait_for,
    SaplingProcesses,
)

REFERENCE_DIR = "reference"
KEY_PREFIX = "reference/"
REFERENCE_UMASK = 0o022
# The reference is a plain filesystem checkout: Sapling never consults watchman
# there, whatever the host enables, so no watchman client outlives a command.
PLAIN_FILESYSTEM = ("--config", "fsmonitor.mode=off")

SETUP = "setup"
UNSUPPORTED = "unsupported"
SETUP_SECONDS = 300.0


class ReferenceFailure(Exception):
    """A reference side or setup step that produced no trustworthy observation."""

    def __init__(self, kind: str, detail: str) -> None:
        super().__init__(f"{kind}: {detail}")
        self.kind = kind
        self.detail = detail


def reference_applicability(spec: RunSpec) -> Applicability:
    """Return whether this run's environment admits a plain reference checkout.

    Spec validation already refuses a differential run on a case-insensitive
    mount; this is the runtime backstop for a spec built without it.
    """
    if not spec.environment.target["case_sensitive"]:
        return not_applicable(CASE_INSENSITIVE_MOUNT)
    return SHARED


def refuse_inapplicable(spec: RunSpec) -> None:
    """Raise ReferenceNotApplicable for a differential run the reference cannot serve."""
    if not reference_clone(spec):
        return
    applicability = reference_applicability(spec)
    if not applicability.shared:
        raise ReferenceNotApplicable(applicability.reason)


SETUP_KILL_WAIT = 10.0


@dataclass(frozen=True)
class InitialState:
    """The modeled starting state: the checked-out commit plus the scattered dirt.

    Each dirt entry is one image written with a single pwrite at offset 0,
    exactly as setup dirties the mount: bytes past the image are kept.
    """

    node: str
    dirt: tuple[tuple[str, bytes], ...]

    def digest(self) -> str:
        """Return a digest naming the commit and every dirty path's bytes."""
        document = {
            "node": self.node,
            "dirt": [
                [path, hashlib.sha256(data).hexdigest()] for path, data in self.dirt
            ],
        }
        text = json.dumps(document, sort_keys=True, separators=(",", ":"))
        return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


class ReferenceRuntime:
    """Paired-action count and typed reference failures of one run, shared by owners."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._paired = 0
        self._failures: list[dict[str, Any]] = []

    def paired(self) -> None:
        """Count one completed pair."""
        with self._lock:
            self._paired += 1

    def failed(
        self,
        action_id: str,
        kind: str,
        detail: str,
        primary: Mapping[str, Any] | None = None,
    ) -> None:
        """Record one typed reference failure and what the primary side returned or raised.

        The action's terminal event records the pair as unresolved, so the
        primary side's own outcome and facts are kept here, whole.
        """
        failure: dict[str, Any] = {"action": action_id, "kind": kind, "detail": detail}
        if primary is not None:
            failure["primary"] = dict(primary)
        with self._lock:
            self._failures.append(failure)

    def snapshot(self) -> dict[str, Any]:
        """Return the paired count and every failure recorded so far."""
        with self._lock:
            return {"paired": self._paired, "failures": list(self._failures)}


class ReferenceCheckout:
    """One plain Sapling checkout and every process that runs in it.

    Its processes follow the one Sapling process rule both sides share
    (``SaplingProcesses``), with umask 022 and their output files beside
    the checkout.
    """

    def __init__(
        self,
        root: Path,
        hg_bin: str,
        hg_environment: Mapping[str, str],
        nodes: Sequence[str],
    ) -> None:
        self._root = root
        self._sl_bin = hg_bin
        self._sl_environment = dict(hg_environment)
        self._nodes = tuple(nodes)
        self._registry = ProcessRegistry()
        self._runtime = ReferenceRuntime()
        self._processes = SaplingProcesses(
            self._registry,
            ReferenceFailure,
            umask=REFERENCE_UMASK,
            scratch=str(root.parent),
        )

    @property
    def root(self) -> Path:
        """Return the checkout's root directory."""
        return self._root

    @property
    def nodes(self) -> tuple[str, ...]:
        """Return the fixture's commits, oldest first."""
        return self._nodes

    @property
    def processes(self) -> ProcessRegistry:
        """Return the registry of reference process groups; EdenFS has its own."""
        return self._registry

    @property
    def runtime(self) -> ReferenceRuntime:
        """Return the run's paired count and typed reference failures."""
        return self._runtime

    def live(self) -> tuple[str, ...]:
        """Return the keys of reference processes not yet finished."""
        return self._processes.live()

    @property
    def token(self) -> str:
        """Return the run token every process token of this checkout begins with."""
        return self._processes.token

    def leftovers(self) -> list[int]:
        """Return every live process carrying a token this checkout issued."""
        return self._processes.leftovers()

    def path(self, relative: str) -> str:
        """Return the reference path of one checkout-relative modeled path."""
        return str(self._root / relative)

    def prepare(self, source: str, state: InitialState) -> dict[str, Any]:
        """Clone ``source`` at the state's commit, replay its dirt, and verify both.

        Returns the reference setup facts. Any step that leaves the checkout
        anywhere but the modeled state raises ReferenceFailure.
        """
        cloned = self.run(
            "setup/clone",
            ["clone", "--updaterev", state.node, source, str(self._root)],
            SETUP_KILL_WAIT,
            cwd=str(self._root.parent),
            until=time.monotonic() + SETUP_SECONDS,
        )
        if cloned.returncode != 0:
            raise ReferenceFailure(SETUP, f"clone exited {cloned.returncode}")
        for path, data in state.dirt:
            written = write_image(self.path(path), data)
            if written.outcome != OK:
                raise ReferenceFailure(SETUP, f"dirt {path}: {written.facts}")
        self._verify(state)
        return {
            "path": str(self._root),
            "node": state.node,
            "dirty_paths": len(state.dirt),
            "initial_state": state.digest(),
            "umask": f"{REFERENCE_UMASK:04o}",
        }

    def _verify(self, state: InitialState) -> None:
        """Require the checkout's parent and status to be exactly the modeled state."""
        parent = self.run(
            "setup/parent",
            ["log", "-r", ".", "-T", "{node}"],
            SETUP_KILL_WAIT,
            until=time.monotonic() + SETUP_SECONDS,
        )
        if parent.returncode != 0 or parent.stdout.strip() != state.node:
            raise ReferenceFailure(SETUP, f"parent {parent.stdout.strip()!r}")
        status = self.run(
            "setup/status",
            ["status", "--print0"],
            SETUP_KILL_WAIT,
            until=time.monotonic() + SETUP_SECONDS,
        )
        if status.returncode != 0:
            raise ReferenceFailure(SETUP, f"status exited {status.returncode}")
        expected = {path: "M" for path, _data in state.dirt}
        observed = parse_status(status.stdout)
        if observed != expected:
            raise ReferenceFailure(
                SETUP, f"status {sorted(observed.items())} != dirt {sorted(expected)}"
            )
        for path, data in state.dirt:
            with open(self.path(path), "rb") as handle:
                if handle.read(len(data)) != data:
                    raise ReferenceFailure(SETUP, f"dirt {path} was not replayed")

    def run(
        self,
        key: str,
        args: Sequence[str],
        kill_wait: float,
        *,
        cwd: str | None = None,
        until: float | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """Run one Sapling process in the checkout, published under ``reference/<key>``.

        ``until`` bounds the leader (``reference-timeout``); every other
        typed failure is the shared process rule's.
        """
        return self._processes.run(
            KEY_PREFIX + key,
            [self._sl_bin, *args, *PLAIN_FILESYSTEM],
            kill_wait,
            cwd=cwd or str(self._root),
            env=self._sl_environment,
            until=until,
        )

    def close_resources(self, deadline: float | None = None) -> int:
        """SIGKILL every live process and descendant, and reap them by the deadline.

        New processes are refused from here on; returns the shared rule's
        leak count, and live processes stay for a later retry.
        """
        return self._processes.close_resources(deadline)


class ReferenceBackend:
    """Run one shared action on the reference checkout, the way LiveBackend runs it on EdenFS.

    Both sides run the same ``SharedExecutor``; red controls, faults, and
    kills never reach this one. Every Sapling process of one action gets
    the operation's ``kill_wait_for`` budget, and its leader is bounded by
    half the operation deadline from the action's first process.
    """

    def __init__(self, checkout: ReferenceCheckout, spec: RunSpec) -> None:
        self._checkout = checkout
        self._spec = spec
        self._kill_wait = kill_wait_for(spec.budgets.operation_deadline_seconds)
        self._leader_seconds = spec.budgets.operation_deadline_seconds / 2
        self._owners = {
            action_id: actor.id for actor in spec.actors for action_id in actor.actions
        }
        self._executor = SharedExecutor(
            checkout.path,
            self._payload,
            lambda revision: checkout.nodes[revision],
            self._run,
            ReferenceFailure,
        )

    @property
    def checkout(self) -> ReferenceCheckout:
        """Return the checkout this backend runs on."""
        return self._checkout

    def __call__(self, action: Action) -> BackendResult:
        """Run one shared action once; an OSError is that side's failed outcome."""
        if not serves(self._spec, action):
            raise ReferenceFailure(
                UNSUPPORTED, f"{action.id}: {action.kind} {dict(action.payload)}"
            )
        return self._executor(action)

    def close_resources(self, deadline: float | None = None) -> int:
        """Close held descriptors, then the checkout's process groups."""
        self._executor.close()
        return self._checkout.close_resources(deadline)

    def _payload(self, action: Action) -> bytes:
        owner = self._owners[action.id]
        return encode_value(self._spec.seed, owner, action.payload["value_id"])

    def _run(
        self, action: Action, args: list[str], *, primary: bool, started: float
    ) -> subprocess.CompletedProcess[str]:
        return self._checkout.run(
            f"{action.id}/{args[0]}",
            args,
            self._kill_wait,
            until=started + self._leader_seconds,
        )
