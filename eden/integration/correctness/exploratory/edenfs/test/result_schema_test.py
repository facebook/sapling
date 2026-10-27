# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Result schemas of every action kind: the ok fields and errno uses each kind declares,
every channel shape a kind may record, strict decoding, and real backend results
(syscalls on a temporary directory, host processes, canned daemon and Sapling
replies) decoding against them. Kinds that need a live daemon for any result are
covered by `backends_scm_test.LiveResultSchemaTest`.
"""

from __future__ import annotations

import errno
import hashlib
import os
import shutil
import subprocess
import tempfile
import time
import unittest
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from typing import Any, cast
from unittest import mock

from eden.fs.service.eden.thrift_types import (
    ChangesSinceV2Result,
    DirListAttributeDataOrError,
    EdenError,
    EdenErrorType,
    FileAttributeDataOrErrorV2,
    FileAttributeDataV2,
    GetScmStatusResult,
    JournalPosition,
    PeekCurrentJournalPositionResponse,
    ScmBlobOrError,
    ScmFileStatus,
    ScmStatus,
    SourceControlType,
    SourceControlTypeOrError,
)
from eden.integration.correctness.exploratory.core.differential import REFERENCE_RESULT
from eden.integration.correctness.exploratory.core.facts import (
    decode_result,
    ErrnoUse,
    NO_ERRNO,
    ResultProblem,
    TypedResult,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Environment as SpecEnvironment,
    RunSpec,
)
from eden.integration.correctness.exploratory.core.scheduler import BackendResult
from eden.integration.correctness.exploratory.core.target import ProcessRegistry
from eden.integration.correctness.exploratory.edenfs.actions import (
    ACTION_DEFS,
    channel_shape_problem,
    CHANNEL_SHAPES,
    DIFFERENTIAL_OBSERVATIONS,
    EDENFS_ACTION_DEFS,
    timeout_capable,
)
from eden.integration.correctness.exploratory.edenfs.backends import LiveBackend
from eden.integration.correctness.exploratory.edenfs.differential_backend import (
    DifferentialBackend,
)
from eden.integration.correctness.exploratory.edenfs.encoding import initial_content
from eden.integration.correctness.exploratory.edenfs.reference import (
    ReferenceBackend,
    ReferenceCheckout,
    ReferenceFailure,
)
from eden.integration.correctness.exploratory.edenfs.sapling_processes import (
    LEFT_RUNNING,
)
from eden.integration.correctness.exploratory.edenfs.target import RECORDING_TARGET
from eden.integration.correctness.exploratory.edenfs.test.actions_fixtures import (
    kind_action,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _CannedSl,
    _FakeMountRepo,
    _spec,
    REGISTERS,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import make_action
from eden.integration.correctness.repo import GeneratedRepo
from thrift.python.exceptions import TransportError, TransportErrorType

_R0, _R1 = REGISTERS
# Kinds whose every result needs a live daemon, or that no backend completes.
LIVE_ONLY = frozenset(
    {
        # The scheduler always ends it `info`; no backend implements it.
        "blocked_faults",
    }
)


class _Env:
    """A mount on a temporary directory with a canned Thrift client and Sapling."""

    def __init__(self, root: Path) -> None:
        self.mount_path = root
        self.processes = ProcessRegistry()
        self.setup_facts: dict[str, Any] = {}
        self.client = mock.MagicMock()
        self.mount_repo = _FakeMountRepo({_R0: "M"}, parent="ab" * 20)
        self.commits = tuple(
            SimpleNamespace(node=f"{index + 1:040x}") for index in range(4)
        )
        self._layout = GeneratedRepo(11, 32, 4)
        self.daemon_pid = os.getpid()

    @contextmanager
    def get_thrift_client(self, timeout: float | None = None) -> Iterator[Any]:
        yield self.client


def _posix(code: int, message: str = "no such file") -> EdenError:
    return EdenError(
        message=message, errorCode=code, errorType=EdenErrorType.POSIX_ERROR
    )


class ResultSchemaFixtureTest(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for register in REGISTERS:
            path = self.root / register
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(initial_content(11, register))
        (self.root / "tree" / "wide").mkdir()
        # Long names spread the listing over several getdents calls, so the
        # stream's pause straddles one.
        for index in range(2000):
            name = f"{index:06d}-entry-whose-long-name-spreads-the-listing.dat"
            (self.root / "tree" / "wide" / name).write_bytes(b"")
        self.env = _Env(self.root)
        self.covered: dict[str, set[str]] = {}

    def _backend(self, actions: list[Action]) -> tuple[LiveBackend, RunSpec]:
        spec = _spec(_R0, actions, [("a", [action.id for action in actions])])
        backend = LiveBackend(cast(Any, self.env), spec)
        self.addCleanup(backend.close_resources)
        return backend, spec

    def _decodes(self, kind: str, result: BackendResult) -> None:
        schema = RECORDING_TARGET.actions[kind].result
        decoded = decode_result(schema, result.outcome, dict(result.facts))
        self.assertNotIsInstance(decoded, ResultProblem, (kind, result))
        self.covered.setdefault(kind, set()).add(result.outcome)

    def _run(
        self, *actions: Action, expect: tuple[str, ...]
    ) -> dict[str, BackendResult]:
        backend, spec = self._backend(list(actions))
        results = {}
        for action, outcome in zip(actions, expect, strict=True):
            result = backend(spec.action(action.id))
            self.assertEqual(outcome, result.outcome, (action.id, result.facts))
            self._decodes(action.kind, result)
            results[action.id] = result
        return results

    def test_every_kind_decodes(self) -> None:
        self._syscalls()
        self._thrift()
        self._processes()
        self._faults()
        self._paired()
        kinds = frozenset(RECORDING_TARGET.actions)
        self.assertEqual(kinds - LIVE_ONLY, frozenset(self.covered))
        for kind in ("write", "read", "getattr", "rename", "unlink", "mkdir"):
            self.assertIn("fail", self.covered[kind], kind)

    def _syscalls(self) -> None:
        moved = "tree/registers/moved.reg"
        self._run(
            make_action("w1", "write", path=_R0, value_id="w1"),
            make_action("r1", "read", path=_R0, channel="fuse"),
            make_action("o1", "open", path=_R0),
            make_action("f1", "read_fd", fd="o1"),
            make_action("c1", "close", fd="o1"),
            make_action("h1", "chmod", path=_R0, mode=0o755),
            make_action("n1", "rename", source=_R1, destination=moved),
            make_action("u1", "unlink", path=moved),
            make_action("m1", "mkdir", path="tree/registers/made"),
            make_action("d1", "readdir", path="tree/registers", channel="fuse"),
            make_action("z1", "sleep", seconds=0.0),
            expect=("ok",) * 11,
        )
        self._run(
            make_action("w2", "write", path=moved, value_id="w2"),
            make_action("w3", "write", path="tree/registers/made", value_id="w3"),
            make_action("r2", "read", path=moved, channel="fuse"),
            make_action("o2", "open", path=moved),
            make_action("h2", "chmod", path=moved, mode=0o644),
            make_action("n2", "rename", source=_R1, destination=moved),
            make_action("u2", "unlink", path="tree/registers/made"),
            make_action("m2", "mkdir", path="tree/registers/made"),
            make_action("d2", "readdir", path=moved, channel="fuse"),
            expect=("fail",) * 9,
        )
        empty = self.root / "tree" / "registers" / "empty"
        empty.write_bytes(b"")
        self._run(
            make_action("r3", "read", path="tree/registers/empty", channel="fuse"),
            expect=("fail",),
        )

    def _thrift(self) -> None:
        client = self.env.client
        client.getFileContent.side_effect = [
            SimpleNamespace(blob=ScmBlobOrError(blob=b"x" * 64)),
            SimpleNamespace(blob=ScmBlobOrError(error=_posix(errno.ENOENT))),
            TransportError(TransportErrorType.END_OF_FILE, "transport", 0, 0),
        ]
        attributes = FileAttributeDataOrErrorV2(
            fileAttributeData=FileAttributeDataV2(
                sourceControlType=SourceControlTypeOrError(
                    sourceControlType=SourceControlType.REGULAR_FILE
                )
            )
        )
        client.getAttributesFromFilesV2.side_effect = [
            SimpleNamespace(res=[attributes]),
            SimpleNamespace(res=[FileAttributeDataOrErrorV2(error=_posix(2))]),
        ]
        client.readdir.side_effect = [
            SimpleNamespace(
                dirLists=[
                    DirListAttributeDataOrError(
                        dirListAttributeData={b"r0.reg": attributes}
                    )
                ]
            ),
            SimpleNamespace(
                dirLists=[DirListAttributeDataOrError(error=_posix(errno.ENOTDIR))]
            ),
        ]
        status = GetScmStatusResult(
            status=ScmStatus(
                entries={b"tree/registers/r0.reg": ScmFileStatus.MODIFIED}, errors={}
            ),
            version="",
        )
        client.getScmStatusV2.side_effect = [
            status,
            status,
            EdenError(
                message="parent is out of date",
                errorType=EdenErrorType.OUT_OF_DATE_PARENT,
            ),
        ]
        client.checkOutRevision.side_effect = [
            [],
            # The daemon's guard refusal.
            EdenError(
                message="another checkout operation is still in progress",
                errorType=EdenErrorType.CHECKOUT_IN_PROGRESS,
            ),
        ]
        client.peekCurrentJournalPosition.return_value = (
            PeekCurrentJournalPositionResponse(
                position=JournalPosition(mountGeneration=7, sequenceNumber=3)
            )
        )
        client.changesSinceV2.return_value = ChangesSinceV2Result(
            toPosition=JournalPosition(mountGeneration=7, sequenceNumber=3), changes=[]
        )
        client.debugInvalidateNonMaterialized.return_value = SimpleNamespace(
            numInvalidated=1
        )
        thrift_read = {"channel": "thrift", "sync_timeout_seconds": 60}
        sl = _CannedSl("ab" * 20, "ab" * 20, "M tree/registers/r0.reg\0")
        with mock.patch.object(subprocess, "Popen", side_effect=sl):
            results = self._thrift_run(thrift_read)
        self._thrift_check(results)
        refused_sl = _CannedSl("refused", returncode=1)
        with mock.patch.object(subprocess, "Popen", side_effect=refused_sl):
            refused = self._run(make_action("q2", "status_counts"), expect=("ok",))
        self.assertEqual({"refused": "refused"}, refused["q2"].facts)

    def _thrift_run(self, thrift_read: dict[str, Any]) -> dict[str, BackendResult]:
        return self._run(
            make_action("t1", "read", path=_R0, **thrift_read),
            make_action("t2", "read", path=_R0, **thrift_read),
            make_action("t3", "read", path=_R0, **thrift_read),
            make_action("g1", "getattr", path=_R0),
            make_action("g2", "getattr", path=_R0),
            make_action("g3", "getattr", path="tree/registers/none"),
            make_action("e1", "readdir", path="tree/registers", channel="thrift"),
            make_action("e2", "readdir", path="tree/registers", channel="thrift"),
            make_action("s1", "status", channel="thrift", revision=1),
            make_action("s2", "status", channel="thrift"),
            make_action("s5", "status", channel="thrift"),
            make_action("k1", "checkout", revision=0, via="thrift", mode="force"),
            make_action("k2", "checkout", revision=1, via="thrift", mode="normal"),
            make_action("j1", "journal_window", since_sequence=0),
            make_action("j2", "journal_window"),
            make_action("p1", "manifest_presence", from_revision=3, to_revision=0),
            make_action("i1", "unload_inodes", path="tree/registers"),
            make_action("q1", "status_counts"),
            expect=("ok", "fail", "fail", "ok", "ok", "fail", "ok", "fail")
            + ("ok", "ok", "fail", "ok", "fail")
            + ("ok",) * 5,
        )

    def _thrift_check(self, results: dict[str, BackendResult]) -> None:
        self.assertEqual(
            {
                "reason": "thrift: no such file",
                "source": "thrift",
                "error_type": "POSIX_ERROR",
                "errno": 20,
            },
            results["e2"].facts,
        )
        self.assertEqual(
            ("thrift", "CHECKOUT_IN_PROGRESS"),
            (results["k2"].facts["source"], results["k2"].facts["error_type"]),
        )
        self.assertEqual(
            (errno.ENOENT, False),
            (results["t2"].facts["error_code"], "errno" in results["t2"].facts),
        )
        self.assertEqual(
            {
                "reason": "thrift: parent is out of date",
                "source": "thrift",
                "error_type": "OUT_OF_DATE_PARENT",
            },
            results["s5"].facts,
        )

    def _processes(self) -> None:
        sl = _CannedSl(
            "M tree/registers/r0.reg\0", returncodes=(0, 0, 255, 0, 255, 0, 1, 0, 1)
        )
        moved = "tree/registers/slmoved.reg"
        with mock.patch.object(subprocess, "Popen", side_effect=sl):
            self._run(
                make_action("s3", "status", channel="sl"),
                make_action("s4", "status", channel="sl", revision=1),
                make_action("k3", "checkout", revision=1, via="goto", mode="normal"),
                make_action("k4", "checkout", revision=0, via="goto", mode="normal"),
                make_action("x1", "sl_rm", path=_R0),
                make_action("x2", "sl_rm", path=_R0),
                make_action("v1", "sl_mv", source=_R1, destination=moved),
                make_action("v2", "sl_mv", source=_R1, destination=moved),
                expect=("ok", "fail", "ok", "fail", "ok", "fail", "ok", "fail"),
            )
        self._run(
            make_action("b1", "background_read", root="tree", seconds=0.2),
            make_action("y1", "readdir_stream", path="tree/wide", pause_after=1000),
            make_action("y2", "readdir_resume", stream="y1"),
            expect=("ok", "ok", "ok"),
        )
        sleeper = make_action("s9", "status", channel="sl")
        killer = make_action("kp", "kill_process", target_action="s9", after_ms=0)
        backend, spec = self._backend([sleeper, killer])
        process = subprocess.Popen(["sleep", "30"], start_new_session=True)
        self.addCleanup(process.wait)
        self.env.processes.publish("s9", process, time.monotonic())
        result = backend(spec.action("kp"))
        self.assertEqual("ok", result.outcome)
        self._decodes("kill_process", result)

    def _faults(self) -> None:
        client = self.env.client
        client.removeFault.return_value = False
        client.getBlockedFaults.side_effect = [
            SimpleNamespace(keyValues=["tree/registers/r0.reg"]),
            SimpleNamespace(keyValues=[]),
        ]
        install = make_action(
            "fi",
            "fault_install",
            fault_id="F",
            site="journal-record",
            target_action="w9",
            behavior="block",
        )
        actions = [
            install,
            make_action("fa", "fault_await", fault_id="F"),
            make_action("fr", "fault_remove", fault_id="F"),
            make_action("w9", "write", path=_R0, value_id="w9"),
        ]
        backend, spec = self._backend(actions)
        for action_id in ("fi", "fa", "fr"):
            result = backend(spec.action(action_id))
            self.assertEqual("ok", result.outcome, (action_id, result.facts))
            self._decodes(spec.action(action_id).kind, result)
        error_install = make_action(
            "ei",
            "fault_install",
            fault_id="E",
            site="journal-record",
            target_action="w8",
            behavior="error",
        )
        actions = [
            error_install,
            make_action("w8", "write", path=_R0, value_id="w8"),
            make_action("ea", "fault_await", fault_id="E"),
        ]
        backend, spec = self._backend(actions)
        for action_id in ("ei", "w8", "ea"):
            result = backend(spec.action(action_id))
            self._decodes(spec.action(action_id).kind, result)
        self.assertNotIn("bytes_written", result.facts)

    def _paired(self) -> None:
        """A paired terminal decodes once the reference side is split off."""
        reference_root = self.root.parent / f"{self.root.name}-reference"
        reference_root.mkdir()
        self.addCleanup(shutil.rmtree, reference_root)
        for register in REGISTERS:
            path = reference_root / register
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(initial_content(11, register))
        read = make_action("pr", "read", path=_R0, channel="fuse")
        stat = make_action("pg", "getattr", path=_R0)
        spec = _spec(_R0, [read, stat], [("a", ["pr", "pg"])])
        target = dict(spec.environment.target)
        target["setup_profile"] = {**target["setup_profile"], "reference_clone": True}
        spec = replace(spec, environment=SpecEnvironment(target=target))
        self.env.client.getAttributesFromFilesV2.side_effect = RuntimeError("down")
        checkout = ReferenceCheckout(reference_root, "/bin/false", {}, ())
        backend = DifferentialBackend(
            LiveBackend(cast(Any, self.env), spec),
            ReferenceBackend(checkout, spec),
            spec,
            runtime=checkout.runtime,
        )
        for action in (read, stat):
            result = backend(action)
            side = result.facts[REFERENCE_RESULT]
            primary = {k: v for k, v in result.facts.items() if k != REFERENCE_RESULT}
            self._decodes(action.kind, BackendResult(result.outcome, primary))
            self._decodes(action.kind, BackendResult(side["outcome"], side["facts"]))
        with mock.patch.object(
            ReferenceBackend,
            "__call__",
            side_effect=ReferenceFailure(LEFT_RUNNING, "x"),
        ):
            result = backend(read)
        side = result.facts[REFERENCE_RESULT]
        self._decodes("read", BackendResult(side["outcome"], side["facts"]))
        backend.close_resources(time.monotonic() + 10)


_OK_FIELDS: dict[str, set[str]] = {
    "write": {"bytes_written"},
    "read": {"bytes_prefix", "length", "sha256"},
    "read_fd": {"bytes_prefix", "length", "sha256"},
    "getattr": {
        "size",
        "mtime",
        "mtime_history",
        # The integral stat mtime the attributes property compares.
        "mtime_ns",
        "mode",
        "fuse_type",
        "thrift_type",
        "thrift_error",
    },
    "open": {"fd"},
    "close": {"fd"},
    "rename": {"source", "destination"},
    "unlink": {"path"},
    "mkdir": {"path"},
    "chmod": {"path", "mode"},
    "readdir": {"channel", "entries"},
    "readdir_stream": {"pgid", "paused_at", "pos_at_pause"},
    "readdir_resume": {"stream", "entries", "pos_at_pause", "pos_at_end"},
    "status": {"status", "diff_paths", "parent"},
    "checkout": {"revision", "node", "via", "parent", "conflicts", "conflict_types"},
    "sl_rm": {"path"},
    "sl_mv": {"source", "destination"},
    "manifest_presence": {"deleted_presence", "added_presence"},
    "journal_window": {
        "mount_generation",
        "sequence_number",
        "since_sequence",
        "changed_paths",
        "truncated",
        "lost_changes",
    },
    "status_counts": {"counts", "sample", "refused"},
    "background_read": {"pgid", "returncode", "stdout", "stderr", "sweeps"},
    "unload_inodes": {"path", "prepared", "num_invalidated"},
}


# The errnos each kind's failure may carry in some state. `none` is a failure
# without an errno: a Thrift EdenError ("read thrift: EdenError
# ENOENT/EISDIR (`error_code`)", "readdir thrift: EdenError, but no errno
# recorded") or a Sapling exit status ("sl_rm: rc!=0 when the path is
# already untracked/removed", "sl_mv: rc!=0 when the source is missing",
# "status sl: rc!=0 while a checkout is in progress or interrupted"), plus
# the live finding that a Thrift status of Sapling's `.` fails
# OUT_OF_DATE_PARENT after a Thrift checkout (`reference_live_test`).
# read_fd, close, and checkout have no state-dependent errno. A path
# operation fails ENOTDIR when its walk meets a file above the path.
_STATE_TABLE: dict[str, set[str]] = {
    "write": {"ENOENT", "EISDIR", "ENOTDIR"},
    "read": {"ENOENT", "EISDIR", "ENOTDIR", "none"},
    "getattr": {"ENOENT", "ENOTDIR"},
    "chmod": {"ENOENT", "ENOTDIR"},
    "open": {"ENOENT", "ENOTDIR"},
    "rename": {"ENOENT", "EISDIR", "ENOTEMPTY", "EEXIST", "ENOTDIR"},
    "unlink": {"ENOENT", "EISDIR", "ENOTDIR"},
    "mkdir": {"EEXIST", "ENOENT", "ENOTDIR"},
    # A Thrift readdir of a regular file fails ARGUMENT_ERROR/EINVAL.
    "readdir": {"ENOENT", "ENOTDIR", "EINVAL", "none"},
    # `read` on a descriptor opened on a directory fails EISDIR; a
    # descriptor whose open failed holds nothing (EBADF).
    "read_fd": {"EISDIR", "EBADF"},
    "sl_rm": {"none"},
    "sl_mv": {"none"},
    "status": {"none"},
    "close": {"EBADF"},
}


# In a differential run, ok/fail results of the shared actions also carry
# `reference_result`.
_SHARED_KINDS = {
    "write",
    "read",
    "read_fd",
    "getattr",
    "open",
    "close",
    "rename",
    "unlink",
    "mkdir",
    "chmod",
    "readdir",
    "status",
    "checkout",
    "sl_rm",
    "sl_mv",
}


def _decode(kind: str, outcome: str, raw: dict[str, Any]) -> Any:
    return decode_result(ACTION_DEFS[kind].result, outcome, raw)


class ChannelShapeTest(unittest.TestCase):
    """Each channel records exactly its own field set."""

    def test_every_channel_shape_is_accepted_and_mixes_are_not(self) -> None:
        cases = (
            (
                "getattr",
                {},
                "ok",
                "size mtime mtime_history mode fuse_type thrift_type",
                True,
            ),
            ("getattr", {}, "ok", "size mtime mtime_history mode fuse_type", False),
            (
                "getattr",
                {},
                "ok",
                "size mtime mtime_history mode fuse_type thrift_type thrift_error",
                False,
            ),
            ("checkout", {"via": "thrift"}, "ok", "revision node via conflicts", True),
            ("checkout", {"via": "thrift"}, "ok", "revision node via", False),
            ("checkout", {"via": "goto"}, "ok", "revision node via", True),
            ("checkout", {"via": "goto"}, "ok", "revision node via parent", True),
            ("checkout", {"via": "goto"}, "ok", "revision node via conflicts", False),
            (
                "checkout",
                {"via": "goto"},
                "fail",
                "reason returncode stdout stderr",
                True,
            ),
            ("checkout", {"via": "goto"}, "fail", "reason source error_type", False),
            ("checkout", {"via": "thrift"}, "fail", "reason source", True),
            (
                "checkout",
                {"via": "thrift"},
                "fail",
                "reason returncode stdout stderr",
                False,
            ),
            (
                "read",
                {"channel": "thrift"},
                "fail",
                "reason source error_type error_code message",
                True,
            ),
            ("read", {"channel": "fuse"}, "fail", "reason source error_code", False),
            ("readdir", {"channel": "fuse"}, "fail", "reason source", False),
            ("status", {"channel": "thrift"}, "ok", "status parent", True),
            ("status", {"channel": "thrift"}, "ok", "status diff_paths", False),
            ("status", {"channel": "thrift"}, "fail", "reason source error_type", True),
            ("status", {"channel": "thrift"}, "fail", "reason returncode", False),
            ("write", {}, "fail", "reason", True),
        )
        for kind, payload, outcome, keys, accepted in cases:
            action = kind_action(kind, **payload)
            fields: dict[str, Any] = dict.fromkeys(keys.split(), 0)
            with self.subTest(kind=kind, payload=payload, outcome=outcome, keys=keys):
                problem = channel_shape_problem(action, outcome, fields)
                self.assertEqual(accepted, problem is None, problem)
                with_reference = {**fields, REFERENCE_RESULT: {}}
                self.assertEqual(
                    problem, channel_shape_problem(action, outcome, with_reference)
                )

    def test_every_variant_fits_its_kind_schema(self) -> None:
        for (kind, outcome), channels in CHANNEL_SHAPES.items():
            declared = {
                spec.name for spec in getattr(ACTION_DEFS[kind].result, outcome)
            }
            for channel, variants in channels.items():
                for variant in variants:
                    with self.subTest(kind=kind, channel=channel, variant=variant):
                        names = {name.rstrip("?") for name in variant.split()}
                        self.assertLessEqual(names, declared)


class ResultSchemaTest(unittest.TestCase):
    def test_ok_fields_are_exactly_the_recorded_fields(self) -> None:
        self.assertEqual(set(EDENFS_ACTION_DEFS), set(_OK_FIELDS))
        for kind, fields in _OK_FIELDS.items():
            with self.subTest(kind=kind):
                declared = {spec.name for spec in ACTION_DEFS[kind].result.ok}
                paired = {REFERENCE_RESULT} if kind in _SHARED_KINDS else set()
                self.assertEqual(fields | paired, declared)
                failing = {spec.name for spec in ACTION_DEFS[kind].result.fail}
                self.assertEqual(bool(paired), REFERENCE_RESULT in failing)
        self.assertEqual(_SHARED_KINDS, set(DIFFERENTIAL_OBSERVATIONS))

    def test_errno_uses_follow_the_state_table(self) -> None:
        for kind, definition in ACTION_DEFS.items():
            errnos = definition.result.errnos
            state = {name for name, use in errnos.items() if use == ErrnoUse.STATE}
            with self.subTest(kind=kind):
                self.assertEqual(_STATE_TABLE.get(kind, set()), state)
                if timeout_capable(
                    cast(RunSpec, None), cast(Any, None), kind_action(kind)
                ):
                    self.assertEqual(ErrnoUse.TIMEOUT, errnos["ETIMEDOUT"])
        for kind in ("read_fd", "close"):
            with self.subTest(kind=kind):
                self.assertNotIn("EIO", ACTION_DEFS[kind].result.errnos)
                self.assertEqual(
                    ErrnoUse.FAULT, ACTION_DEFS[kind].result.errnos["ENOTCONN"]
                )

    def test_results_decode_strictly(self) -> None:
        digest = {
            "bytes_prefix": "61" * 64,
            "length": 64,
            "sha256": hashlib.sha256(b"a" * 64).hexdigest(),
        }
        accepted = (
            ("write", "ok", {"bytes_written": 64}),
            ("write", "fail", {"reason": "pwrite", "errno": 5, "close_errno": 107}),
            ("write", "fail", {"reason": "short write 3/64"}),
            ("read", "ok", digest),
            (
                "read",
                "fail",
                {
                    "reason": "thrift",
                    "source": "thrift",
                    "error_type": "POSIX_ERROR",
                    "error_code": 2,
                    "message": "x",
                },
            ),
            (
                "getattr",
                "ok",
                {
                    "size": 64,
                    "mtime": 1.5,
                    "mtime_history": 2.5,
                    "mode": 420,
                    "fuse_type": "file",
                    "thrift_type": "file",
                },
            ),
            (
                "status",
                "ok",
                {"status": [{"path": "a", "code": "M"}], "diff_paths": ["a"]},
            ),
            ("status", "ok", {"status": [], "parent": "ab"}),
            (
                "checkout",
                "ok",
                {"revision": 1, "node": "ab", "via": "thrift", "conflicts": 0},
            ),
            (
                "checkout",
                "fail",
                {
                    "reason": "x",
                    "source": "thrift",
                    "error_type": "CHECKOUT_IN_PROGRESS",
                },
            ),
            (
                "checkout",
                "fail",
                {"reason": "x", "returncode": -9, "stdout": "", "stderr": ""},
            ),
            (
                "manifest_presence",
                "ok",
                {
                    "deleted_presence": [{"path": "a", "present": False}],
                    "added_presence": [],
                },
            ),
            (
                "journal_window",
                "ok",
                {
                    "mount_generation": 1,
                    "sequence_number": 3,
                    "since_sequence": 0,
                    "changed_paths": [
                        {"path": "a", "existed_before": True, "existed_after": True}
                    ],
                    "truncated": False,
                    "lost_changes": [],
                },
            ),
            (
                "status_counts",
                "ok",
                {"counts": [{"code": "M", "count": 2}], "sample": ["M a"]},
            ),
            ("read", "info", {"reason": "operation-timeout"}),
        )
        for kind, outcome, raw in accepted:
            with self.subTest(kind=kind, raw=raw):
                self.assertIsInstance(_decode(kind, outcome, raw), TypedResult)
        rejected = (
            ("write", "ok", {}),
            ("write", "ok", {"bytes_written": True}),
            ("read", "ok", {**digest, "extra": 1}),
            ("status", "ok", {"status": {"a": "M"}}),
            ("rename", "fail", {"reason": "x"}),
            ("checkout", "fail", {"reason": "x", "facts": {}}),
            ("read", "info", {"reason": "x", "bytes_prefix": ""}),
        )
        for kind, outcome, raw in rejected:
            with self.subTest(kind=kind, raw=raw):
                self.assertIsInstance(_decode(kind, outcome, raw), ResultProblem)

    def test_modeled_kinds_declare_errno_or_return_codes(self) -> None:
        for kind, definition in ACTION_DEFS.items():
            fail = {spec.name for spec in definition.result.fail}
            with self.subTest(kind=kind):
                if set(definition.result.errnos) - {NO_ERRNO}:
                    self.assertIn("errno", fail)
                if definition.modeled and kind not in (
                    "readdir_stream",
                    "readdir_resume",
                    "status",
                ):
                    self.assertTrue({"errno", "returncode"} & fail, fail)
