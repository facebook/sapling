# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Tests for the backend's source-control and Thrift channels over fake environments."""

import errno
import hashlib
import os
import subprocess
import tempfile
import time
import unittest
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from types import SimpleNamespace
from typing import Any, cast
from unittest import mock

from eden.fs.service.eden import thrift_types as journal_types
from eden.fs.service.eden.thrift_types import (
    CheckoutConflict,
    ConflictType,
    DebugGetRawJournalResponse,
    DirListAttributeDataOrError,
    EdenError,
    EdenErrorType,
    GetFileContentResponse,
    GetScmStatusResult,
    JournalPosition,
    PeekCurrentJournalPositionResponse,
    ScmBlobOrError,
    ScmFileStatus,
    ScmStatus,
)
from eden.integration.correctness.exploratory.core.containment import COMPLETED
from eden.integration.correctness.exploratory.core.history import FAIL, InMemorySink, OK
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    DEADLINE_OPERATION,
    RunSpec,
)
from eden.integration.correctness.exploratory.core.scheduler import (
    BackendResult,
    HarnessFailure,
    INVALID_BACKEND_RESPONSE,
    run_actions,
)
from eden.integration.correctness.exploratory.edenfs.backends import LiveBackend
from eden.integration.correctness.exploratory.edenfs.environment import Environment
from eden.integration.correctness.exploratory.edenfs.executor import SharedExecutor
from eden.integration.correctness.exploratory.edenfs.file_results import (
    parse_diff_stat_paths,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _CannedSl,
    _FakeEnv,
    _native_spec,
    _spec,
    _status_spec,
    decode_problems,
    facts_of,
    REGISTERS,
)
from eden.thrift.client import EdenNotRunningError
from thrift.python.exceptions import (
    ApplicationError,
    ApplicationErrorType,
    TransportError,
    TransportErrorType,
)


def _status_map(facts: Any) -> dict[str, str]:
    """Return a status result's entry list as a path-to-code map."""
    return {entry["path"]: entry["code"] for entry in facts["status"]}


def _changed_map(facts: Any) -> dict[str, dict[str, bool]]:
    """Return a journal window's changed-path entries keyed by path."""
    return {
        entry["path"]: {k: v for k, v in entry.items() if k != "path"}
        for entry in facts["changed_paths"]
    }


class _FakeThriftClient:
    def __init__(self, response: GetScmStatusResult, *, fail: bool = False) -> None:
        self.response = response
        self.fail = fail
        self.error: EdenError | None = None
        self.requests: list[Any] = []

    def getScmStatusV2(self, request: Any) -> GetScmStatusResult:
        self.requests.append(request)
        if self.fail:
            raise RuntimeError("rpc failed")
        if self.error is not None:
            raise self.error
        return self.response


class _FakeThriftEnv(_FakeEnv):
    def __init__(self, response: GetScmStatusResult) -> None:
        super().__init__({}, parent="aa" * 20)
        self.mount_path = "/mount"
        self.commits = cast(
            Any, tuple(mock.Mock(node=f"{i + 1:040x}") for i in range(4))
        )
        self.client = _FakeThriftClient(response)

    @contextmanager
    def get_thrift_client(self) -> Iterator[_FakeThriftClient]:
        yield self.client


def _probe_spec() -> RunSpec:
    """One sl status probe over the first register path."""
    register = REGISTERS[0]
    probe = Action(
        id="s1",
        kind="status",
        deadline=DEADLINE_OPERATION,
        payload={"channel": "sl"},
    )
    return _spec(register, [probe], [("a", ["s1"])])


class ScmStatusBackendTest(unittest.TestCase):
    def _run_probe(
        self,
        status: dict[str, str],
        parent: str | None = "abc123",
        raise_status: bool = False,
        red_control: str | None = None,
        diff: str = "",
        raise_diff: bool = False,
    ) -> Any:
        spec = _probe_spec()
        backend = LiveBackend(
            _FakeEnv(status, parent, raise_status, diff, raise_diff),  # type: ignore[arg-type]
            spec,
            red_control,
        )
        output = "".join(f"{flag} {path}\0" for path, flag in status.items())
        sl = _CannedSl(
            output,
            diff,
            returncodes=(1 if raise_status else 0, 1 if raise_diff else 0),
        )
        with mock.patch.object(subprocess, "Popen", side_effect=sl):
            return backend(spec.action("s1"))

    def test_reports_status(self) -> None:
        result = self._run_probe({REGISTERS[0]: "M"})
        self.assertEqual(result.outcome, OK)
        self.assertEqual(_status_map(result.facts), {REGISTERS[0]: "M"})
        self.assertEqual(result.facts["diff_paths"], [])
        self.assertNotIn("parent", result.facts)

    def test_reports_diff_paths(self) -> None:
        diff = (
            f" {REGISTERS[0]} | 1 +-\n"
            f" {REGISTERS[1]} | 2 ++--\n"
            " 2 files changed, 2 insertions(+), 2 deletions(-)\n"
        )
        result = self._run_probe({REGISTERS[0]: "M", REGISTERS[1]: "M"}, diff=diff)
        self.assertEqual(result.outcome, OK)
        self.assertEqual(result.facts["diff_paths"], [REGISTERS[0], REGISTERS[1]])

    def test_red_loss_drops_first_modified(self) -> None:
        result = self._run_probe(
            {REGISTERS[1]: "M", REGISTERS[0]: "M"}, red_control="status-loss"
        )
        self.assertEqual(result.outcome, OK)
        self.assertEqual(_status_map(result.facts), {REGISTERS[1]: "M"})

    def test_red_status_loss_preserves_diff_evidence(self) -> None:
        """The status-loss control promises one truthful live comparison channel."""
        diff = f" {REGISTERS[0]} | 1 +-\n 1 file changed, 1 insertion(+)\n"
        result = self._run_probe(
            {REGISTERS[0]: "M"}, red_control="status-loss", diff=diff
        )
        self.assertEqual(result.outcome, OK)
        self.assertEqual(_status_map(result.facts), {})
        self.assertEqual(result.facts["diff_paths"], [REGISTERS[0]])

    def test_red_diff_loss_drops_diff_only(self) -> None:
        diff = f" {REGISTERS[0]} | 1 +-\n 1 file changed, 1 insertion(+)\n"
        result = self._run_probe(
            {REGISTERS[0]: "M"}, red_control="diff-loss", diff=diff
        )
        self.assertEqual(result.outcome, OK)
        self.assertEqual(_status_map(result.facts), {REGISTERS[0]: "M"})
        self.assertEqual(result.facts["diff_paths"], [])

    def test_red_common_status_loss_drops_the_path_from_status_and_diff(self) -> None:
        """Both channels on the shared Sapling path lose the same modified path."""
        diff = f" {REGISTERS[0]} | 1 +-\n {REGISTERS[1]} | 1 +-\n"
        result = self._run_probe(
            {REGISTERS[1]: "M", REGISTERS[0]: "M"},
            red_control="common-status-loss",
            diff=diff,
        )
        self.assertEqual(result.outcome, OK)
        self.assertEqual(_status_map(result.facts), {REGISTERS[1]: "M"})
        self.assertEqual(result.facts["diff_paths"], [REGISTERS[1]])

    def test_common_status_loss_history_records_both_losses(self) -> None:
        """Shared-path loss drops the path from status and diff; the history decodes."""
        register = REGISTERS[0]
        spec = _status_spec(register)
        backend = LiveBackend(
            _FakeEnv({}),  # type: ignore[arg-type]
            spec,
            "common-status-loss",
        )
        sl = _CannedSl(f"M {register}\0", f" {register} | 1 +-\n")
        written = BackendResult(OK, {"bytes_written": 64})
        with (
            mock.patch.object(subprocess, "Popen", side_effect=sl),
            mock.patch.object(SharedExecutor, "_write", return_value=written),
        ):
            sink = InMemorySink()
            status = run_actions(spec, backend, sink, time.monotonic)
        self.assertEqual(COMPLETED, status.status)
        probe = next(
            e for e in sink.events if e.data.get("action") == "s1" and e.type == OK
        )
        self.assertEqual(([], []), (probe.data["status"], probe.data["diff_paths"]))
        self.assertEqual({}, decode_problems(facts_of(spec, sink.events)))

    def test_red_loss_clean_stays_clean(self) -> None:
        result = self._run_probe({}, red_control="status-loss")
        self.assertEqual(result.outcome, OK)
        self.assertEqual(_status_map(result.facts), {})

    def test_sl_error_fails_probe(self) -> None:
        result = self._run_probe({REGISTERS[0]: "M"}, raise_status=True)
        self.assertEqual(result.outcome, FAIL)

    def test_diff_error_fails_probe(self) -> None:
        result = self._run_probe({REGISTERS[0]: "M"}, raise_diff=True)
        self.assertEqual(result.outcome, FAIL)


class DirstateBackendTest(unittest.TestCase):
    """sl_rm and sl_mv each run one sl process in its own group, like status."""

    def _run(self, action: Action, returncode: int = 0) -> tuple[Any, list[str]]:
        spec = _native_spec(action)
        backend = LiveBackend(_FakeEnv({}), spec)  # type: ignore[arg-type]
        sl = _CannedSl(returncode=returncode)
        with mock.patch.object(subprocess, "Popen", side_effect=sl) as popen:
            result = backend(action)
        (command,), kwargs = popen.call_args
        self.assertTrue(kwargs["start_new_session"])
        return result, command[2:]

    def test_dirstate_kinds_run_sl_rm_force_and_sl_mv(self) -> None:
        """sl_rm forces removal, sl_mv moves with copy tracking; a nonzero exit fails."""
        moved, source = "tree/registers/moved.reg", REGISTERS[0]
        cases = (
            ("sl_rm", {"path": source}, ["rm", "--force", source]),
            ("sl_mv", {"source": source, "destination": moved}, ["mv", source, moved]),
        )
        for kind, payload, expected in cases:
            with self.subTest(kind=kind):
                action = Action("h0", kind, DEADLINE_OPERATION, payload=payload)
                result, args = self._run(action)
                self.assertEqual((OK, payload), (result.outcome, result.facts))
                self.assertEqual(expected, args)
                failed, _args = self._run(action, returncode=1)
                self.assertEqual(FAIL, failed.outcome)
                self.assertEqual(1, failed.facts["returncode"])


class SlCheckoutBackendTest(unittest.TestCase):
    """`sl goto`/`sl goto` run one sl process; `continue` resumes `.hg/updatestate`."""

    def test_normal_names_the_node_and_continue_and_force_pass_their_flag(
        self,
    ) -> None:
        node = f"{2:040x}"
        for mode, expected in (
            ("normal", [node]),
            ("continue", ["--continue"]),
            ("force", ["--clean", node]),
        ):
            with self.subTest(mode=mode):
                action = Action(
                    "c0",
                    "checkout",
                    DEADLINE_OPERATION,
                    payload={"revision": 1, "via": "goto", "mode": mode},
                )
                env = _FakeEnv({})
                env.commits = cast(
                    Any, tuple(mock.Mock(node=f"{i + 1:040x}") for i in range(4))
                )
                backend = LiveBackend(env, _native_spec(action))  # type: ignore[arg-type]
                with mock.patch.object(
                    subprocess, "Popen", side_effect=_CannedSl()
                ) as popen:
                    result = backend(action)
                (command,), _kwargs = popen.call_args
                self.assertEqual(OK, result.outcome)
                self.assertEqual(["goto", *expected], command[2:])


class ThriftScmStatusBackendTest(unittest.TestCase):
    def _response(self) -> GetScmStatusResult:
        return GetScmStatusResult(
            status=ScmStatus(
                entries={
                    os.fsencode(REGISTERS[0]): ScmFileStatus.MODIFIED,
                    os.fsencode(REGISTERS[1]): ScmFileStatus.ADDED,
                },
                errors={},
            ),
            version="",
        )

    def test_calls_once_with_explicit_commit_and_decodes_entries(self) -> None:
        action = Action(
            id="s1",
            kind="status",
            deadline=DEADLINE_OPERATION,
            payload={"channel": "thrift", "revision": 1},
        )
        spec = _native_spec(action)
        env = _FakeThriftEnv(self._response())
        with mock.patch.object(
            subprocess, "Popen", side_effect=AssertionError("must not spawn")
        ) as popen:
            result = LiveBackend(cast(Any, env), spec)(action)
        self.assertEqual(result.outcome, OK)
        self.assertEqual(
            _status_map(result.facts),
            {REGISTERS[0]: "MODIFIED", REGISTERS[1]: "ADDED"},
        )
        self.assertEqual(len(env.client.requests), 1)
        self.assertEqual(env.client.requests[0].mountPoint, b"/mount")
        self.assertEqual(env.client.requests[0].commit, bytes.fromhex(f"{2:040x}"))
        popen.assert_not_called()
        self.assertIsNone(env.processes.lookup(action.id))

    def test_omitted_revision_uses_current_parent(self) -> None:
        action = Action(
            id="s1",
            kind="status",
            deadline=DEADLINE_OPERATION,
            payload={"channel": "thrift"},
        )
        spec = _native_spec(action)
        env = _FakeThriftEnv(self._response())
        backend = LiveBackend(cast(Any, env), spec)
        with mock.patch.object(
            subprocess, "Popen", side_effect=_CannedSl("aa" * 20)
        ) as popen:
            result = backend(action)
        self.assertEqual(result.outcome, OK)
        self.assertEqual(len(env.client.requests), 1)
        self.assertEqual(env.client.requests[0].commit, bytes.fromhex("aa" * 20))
        (command,), kwargs = popen.call_args
        self.assertEqual(["log", "-r", ".", "-T", "{node}"], command[2:])
        self.assertTrue(kwargs["start_new_session"])
        self.assertIsNone(env.processes.lookup("s1/log"))

    def test_transport_and_invalid_parent_are_harness_failures(self) -> None:
        action = Action(
            id="s1",
            kind="status",
            deadline=DEADLINE_OPERATION,
            payload={"channel": "thrift"},
        )
        spec = _native_spec(action)
        invalid_parent = _FakeThriftEnv(self._response())
        transport_error = _FakeThriftEnv(self._response())
        transport_error.client.fail = True
        malformed_response = _FakeThriftEnv(cast(Any, object()))
        for env, parent in (
            (invalid_parent, "not-a-node"),
            (transport_error, "aa" * 20),
            (malformed_response, "aa" * 20),
        ):
            with (
                self.subTest(parent=parent),
                mock.patch.object(subprocess, "Popen", side_effect=_CannedSl(parent)),
            ):
                with self.assertRaises(HarnessFailure) as context:
                    LiveBackend(cast(Any, env), spec)(action)
                self.assertEqual(context.exception.reason, INVALID_BACKEND_RESPONSE)

    def test_an_eden_error_is_the_actions_typed_failure(self) -> None:
        """A status of a parent EdenFS has moved past fails with its error type."""
        action = Action(
            id="s1",
            kind="status",
            deadline=DEADLINE_OPERATION,
            payload={"channel": "thrift"},
        )
        env = _FakeThriftEnv(self._response())
        env.client.error = EdenError(
            message="parent is out of date",
            errorType=EdenErrorType.OUT_OF_DATE_PARENT,
        )
        with mock.patch.object(subprocess, "Popen", side_effect=_CannedSl("aa" * 20)):
            result = LiveBackend(cast(Any, env), _native_spec(action))(action)
        self.assertEqual(
            BackendResult(
                FAIL,
                {
                    "reason": "thrift: parent is out of date",
                    "source": "thrift",
                    "error_type": "OUT_OF_DATE_PARENT",
                },
            ),
            result,
        )


class JournalWindowBackendTest(unittest.TestCase):
    def setUp(self) -> None:
        """Build a journal observation followed by sleep over canned RPCs."""
        self.action = Action(
            "j1", "journal_window", DEADLINE_OPERATION, payload={"since_sequence": 0}
        )
        successor = Action("d1", "sleep", DEADLINE_OPERATION, payload={"seconds": 0.0})
        self.spec = _spec(REGISTERS[0], [self.action, successor], [("a", ["j1", "d1"])])
        self.client = mock.Mock()
        self.client.peekCurrentJournalPosition.return_value = (
            PeekCurrentJournalPositionResponse(
                position=JournalPosition(mountGeneration=7, sequenceNumber=1)
            )
        )
        self.client.debugGetRawJournal.return_value = DebugGetRawJournalResponse(
            allDeltas=[]
        )
        self.client.changesSinceV2.return_value = journal_types.ChangesSinceV2Result(
            toPosition=JournalPosition(mountGeneration=7, sequenceNumber=1), changes=[]
        )
        self.env = mock.MagicMock(spec=Environment)
        self.env.mount_path = "/mount"
        self.env.get_thrift_client.return_value.__enter__.return_value = self.client
        self.backend = LiveBackend(self.env, self.spec)

    def _stopped_socket_error(self) -> EdenNotRunningError:
        """Preserve the real exception types raised for a stopped daemon socket."""
        return EdenNotRunningError(
            "/eden/socket",
            TransportError(
                TransportErrorType.NOT_OPEN, "connection refused", errno.ECONNREFUSED, 0
            ),
        )

    def test_journal_transport_and_service_failures_are_action_failures(self) -> None:
        """Unavailable daemon RPCs produce FAIL receipts at either journal stage."""
        errors = (
            self._stopped_socket_error(),
            TransportError(
                TransportErrorType.NOT_OPEN, "connection refused", errno.ECONNREFUSED, 0
            ),
            EdenError(
                message="service shutting down", errorType=EdenErrorType.SHUTTING_DOWN
            ),
        )
        for method in ("peekCurrentJournalPosition", "changesSinceV2"):
            for error in errors:
                with self.subTest(method=method, error=type(error).__name__):
                    with mock.patch.object(self.client, method, side_effect=error):
                        result = self.backend(self.action)
                    self.assertEqual(
                        result,
                        BackendResult(FAIL, {"reason": str(error), "source": "thrift"}),
                    )

    def test_journal_invalid_responses_remain_harness_failures(self) -> None:
        """Malformed replies and unexpected implementation errors are not FAIL data."""
        for method in ("peekCurrentJournalPosition", "changesSinceV2"):
            for response, error in (
                (object(), None),
                (None, RuntimeError("unexpected journal failure")),
            ):
                with self.subTest(method=method, response=response):
                    with mock.patch.object(
                        self.client, method, return_value=response, side_effect=error
                    ):
                        with self.assertRaises(HarnessFailure) as caught:
                            self.backend(self.action)
                    self.assertEqual(INVALID_BACKEND_RESPONSE, caught.exception.reason)

    def test_journal_generated_responses_return_facts(self) -> None:
        """Valid generated Thrift responses remain successful observations."""
        self.assertEqual(
            BackendResult(
                OK,
                {
                    "mount_generation": 7,
                    "sequence_number": 1,
                    "since_sequence": 0,
                    "changed_paths": [],
                    "truncated": False,
                    "lost_changes": [],
                },
            ),
            self.backend(self.action),
        )

    def test_journal_loss_reasons_and_response_cursor_are_preserved(self) -> None:
        """The API's explicit lostChanges reason determines whether paths are complete."""
        for reason in journal_types.LostChangesReason:
            with self.subTest(reason=reason):
                self.client.changesSinceV2.return_value = (
                    journal_types.ChangesSinceV2Result(
                        toPosition=JournalPosition(mountGeneration=8, sequenceNumber=9),
                        changes=[
                            journal_types.ChangeNotification(
                                largeChange=journal_types.LargeChangeNotification(
                                    lostChanges=journal_types.LostChanges(reason=reason)
                                )
                            )
                        ],
                    )
                )
                facts = self.backend(self.action).facts
                self.assertEqual(
                    (8, 9), (facts["mount_generation"], facts["sequence_number"])
                )
                self.assertEqual([reason.name], facts["lost_changes"])
                self.assertEqual(
                    reason == journal_types.LostChangesReason.JOURNAL_TRUNCATED,
                    facts["truncated"],
                )
        request = self.client.changesSinceV2.call_args.args[0]
        self.assertEqual(
            (7, 0),
            (request.fromPosition.mountGeneration, request.fromPosition.sequenceNumber),
        )

    def test_journal_changes_include_paths_without_inventing_commit_paths(self) -> None:
        """Small changes name their paths; a commit transition contains no path list."""
        changes = [
            journal_types.ChangeNotification(
                smallChange=journal_types.SmallChangeNotification(
                    modified=journal_types.Modified(path=b"r")
                )
            ),
            journal_types.ChangeNotification(
                largeChange=journal_types.LargeChangeNotification(
                    commitTransition=journal_types.CommitTransition()
                )
            ),
        ]
        self.client.changesSinceV2.return_value = journal_types.ChangesSinceV2Result(
            toPosition=JournalPosition(mountGeneration=7, sequenceNumber=9),
            changes=changes,
        )
        facts = self.backend(self.action).facts
        self.assertEqual(
            [{"path": "r", "existed_before": True, "existed_after": True}],
            facts["changed_paths"],
        )
        self.assertEqual([], facts["lost_changes"])
        for small, paths in (
            (
                journal_types.SmallChangeNotification(
                    added=journal_types.Added(path=b"a")
                ),
                {"a"},
            ),
            (
                journal_types.SmallChangeNotification(
                    removed=journal_types.Removed(path=b"a")
                ),
                {"a"},
            ),
            (
                journal_types.SmallChangeNotification(
                    renamed=journal_types.Renamed(from_=b"a", to=b"b")
                ),
                {"a", "b"},
            ),
            (
                journal_types.SmallChangeNotification(
                    replaced=journal_types.Replaced(from_=b"a", to=b"b")
                ),
                {"a", "b"},
            ),
        ):
            change = journal_types.ChangeNotification(smallChange=small)
            self.client.changesSinceV2.return_value = (
                journal_types.ChangesSinceV2Result(
                    toPosition=JournalPosition(mountGeneration=7, sequenceNumber=9),
                    changes=[change],
                )
            )
            self.assertEqual(paths, set(_changed_map(self.backend(self.action).facts)))
        self.client.changesSinceV2.return_value = journal_types.ChangesSinceV2Result(
            toPosition=JournalPosition(mountGeneration=7, sequenceNumber=9),
            changes=[journal_types.ChangeNotification()],
        )
        with self.assertRaises(HarnessFailure):
            self.backend(self.action)
        self.client.changesSinceV2.return_value = journal_types.ChangesSinceV2Result(
            toPosition=JournalPosition(mountGeneration=8, sequenceNumber=9), changes=[]
        )
        with self.assertRaises(HarnessFailure):
            self.backend(self.action)
        self.client.changesSinceV2.reset_mock()
        cursor = self.backend(Action("cursor", "journal_window", "operation"))
        self.assertEqual({"mount_generation": 7, "sequence_number": 1}, cursor.facts)
        self.client.changesSinceV2.assert_not_called()

    def test_journal_connection_failure_does_not_skip_later_actions(self) -> None:
        """A failed journal action must not stop the scheduler before later actions."""
        self.env.get_thrift_client.return_value.__enter__.side_effect = (
            self._stopped_socket_error()
        )
        sink = InMemorySink()
        result = run_actions(self.spec, self.backend, sink, time.monotonic)
        self.assertEqual((COMPLETED, None), (result.status, result.reason))
        completions = {
            event.data["action"]: event.type
            for event in sink.events
            if event.type in (OK, FAIL)
        }
        self.assertEqual({"j1": FAIL, "d1": OK}, completions)


class DiffStatParseTest(unittest.TestCase):
    def test_parses_file_column(self) -> None:
        text = (
            " tree/churn/c0 | 1 +-\n"
            " tree/registers/r0.reg | 2 ++\n"
            " 2 files changed, 2 insertions(+), 1 deletion(-)\n"
        )
        self.assertEqual(
            parse_diff_stat_paths(text),
            ["tree/churn/c0", "tree/registers/r0.reg"],
        )

    def test_skips_summary_and_blanks(self) -> None:
        self.assertEqual(
            parse_diff_stat_paths(" 1 file changed, 1 insertion(+)\n\n   \n"),
            [],
        )

    def test_empty_is_empty(self) -> None:
        self.assertEqual(parse_diff_stat_paths(""), [])


class ThriftCheckoutBackendTest(unittest.TestCase):
    def setUp(self) -> None:
        """Build one Thrift checkout over a canned checkOutRevision RPC."""
        self.action = Action(
            "c1",
            "checkout",
            DEADLINE_OPERATION,
            payload={"revision": 1, "via": "thrift", "mode": "normal"},
        )
        self.spec = _spec(REGISTERS[0], [self.action], [("a", ["c1"])])
        self.client = mock.Mock()
        self.client.checkOutRevision.return_value = []
        self.env = mock.MagicMock(spec=Environment)
        self.env.mount_path = "/mount"
        self.env.commits = (SimpleNamespace(node="ab" * 20),) * 2
        self.env.get_thrift_client.return_value.__enter__.return_value = self.client
        self.backend = LiveBackend(self.env, self.spec)

    def test_checkout_transport_and_service_failures_are_action_failures(self) -> None:
        """A refused or unreachable checkout is FAIL data, as journal_window records."""
        errors = (
            EdenNotRunningError(
                "/eden/socket",
                TransportError(
                    TransportErrorType.NOT_OPEN, "refused", errno.ECONNREFUSED, 0
                ),
            ),
            TransportError(TransportErrorType.END_OF_FILE, "eof", 0, 0),
            EdenError(
                message="another checkout operation is still in progress",
                errorType=EdenErrorType.CHECKOUT_IN_PROGRESS,
            ),
        )
        for error in errors:
            with self.subTest(error=type(error).__name__):
                with mock.patch.object(
                    self.client, "checkOutRevision", side_effect=error
                ):
                    result = self.backend(self.action)
                facts = {"reason": str(error), "source": "thrift"}
                if isinstance(error, EdenError):
                    facts["error_type"] = "CHECKOUT_IN_PROGRESS"
                self.assertEqual(BackendResult(FAIL, facts), result)

    def test_checkout_invalid_responses_remain_harness_failures(self) -> None:
        """Malformed replies and unexpected implementation errors are not FAIL data."""
        for response, error in ((object(), None), (None, RuntimeError("boom"))):
            with self.subTest(response=response, error=error):
                with mock.patch.object(
                    self.client,
                    "checkOutRevision",
                    return_value=response,
                    side_effect=error,
                ):
                    with self.assertRaises(HarnessFailure) as caught:
                        self.backend(self.action)
                self.assertEqual(INVALID_BACKEND_RESPONSE, caught.exception.reason)

    def test_checkout_generated_response_returns_facts(self) -> None:
        """A conflict list from the daemon completes ok with its length."""
        self.assertEqual(
            BackendResult(
                OK,
                {
                    "revision": 1,
                    "node": "ab" * 20,
                    "via": "thrift",
                    "conflicts": 0,
                    "conflict_types": [],
                },
            ),
            self.backend(self.action),
        )

    def test_checkout_records_sorted_conflict_types_beside_the_count(self) -> None:
        """Each reported conflict's path and ConflictType name, sorted by path."""
        self.client.checkOutRevision.return_value = [
            CheckoutConflict(
                path=b"tree/registers/r1.reg",
                type=ConflictType.MODIFIED_REMOVED,
                message="",
            ),
            CheckoutConflict(
                path=b"tree/registers/r0.reg",
                type=ConflictType.UNTRACKED_ADDED,
                message="",
            ),
        ]
        result = self.backend(self.action)
        self.assertEqual(OK, result.outcome)
        self.assertEqual(2, result.facts["conflicts"])
        self.assertEqual(
            [
                {"path": "tree/registers/r0.reg", "type": "UNTRACKED_ADDED"},
                {"path": "tree/registers/r1.reg", "type": "MODIFIED_REMOVED"},
            ],
            result.facts["conflict_types"],
        )


class ThriftReadBackendTest(unittest.TestCase):
    def test_read_error_reply_records_the_error_type_name(self) -> None:
        """A Thrift read's error fact uses the bare enum name, as checkout does."""
        action = Action(
            "r1",
            "read",
            DEADLINE_OPERATION,
            payload={
                "path": REGISTERS[0],
                "channel": "thrift",
                "sync_timeout_seconds": 0,
            },
        )
        client = mock.Mock()
        client.getFileContent.return_value = GetFileContentResponse(
            blob=ScmBlobOrError(
                error=EdenError(
                    message="no such file",
                    errorCode=errno.ENOENT,
                    errorType=EdenErrorType.GENERIC_ERROR,
                )
            )
        )
        env = mock.MagicMock(spec=Environment)
        env.mount_path = "/mount"
        env.get_thrift_client.return_value.__enter__.return_value = client
        spec = _spec(REGISTERS[0], [action], [("a", ["r1"])])

        result = LiveBackend(env, spec)(action)

        self.assertEqual(FAIL, result.outcome)
        self.assertEqual("GENERIC_ERROR", result.facts["error_type"])
        self.assertEqual(errno.ENOENT, result.facts["error_code"])
        self.assertNotIn("errno", result.facts)

    def _read(self, **client_behavior: Any) -> BackendResult:
        action = Action(
            "r1",
            "read",
            DEADLINE_OPERATION,
            payload={
                "path": REGISTERS[0],
                "channel": "thrift",
                "sync_timeout_seconds": 0,
            },
        )
        client = mock.Mock()
        client.getFileContent.configure_mock(**client_behavior)
        env = mock.MagicMock(spec=Environment)
        env.mount_path = "/mount"
        env.get_thrift_client.return_value.__enter__.return_value = client
        return LiveBackend(env, _spec(REGISTERS[0], [action], [("a", ["r1"])]))(action)

    def test_errno_less_error_omits_error_code(self) -> None:
        """An EdenError without errorCode (an injected runtime_error) records none.

        `newEdenError(const std::exception&)` builds a GENERIC_ERROR with no
        errorCode for a non-system error.
        """
        result = self._read(
            return_value=GetFileContentResponse(
                blob=ScmBlobOrError(
                    error=EdenError(
                        message="exploratory fault F",
                        errorType=EdenErrorType.GENERIC_ERROR,
                    )
                )
            )
        )
        self.assertEqual(FAIL, result.outcome)
        self.assertEqual("GENERIC_ERROR", result.facts["error_type"])
        self.assertNotIn("error_code", result.facts)

    def test_only_daemon_rpc_failures_are_fail_data(self) -> None:
        """A transport or application error is FAIL data; a harness bug raises."""
        for error in (
            TransportError(TransportErrorType.TIMED_OUT, "timed out", 0, 0),
            ApplicationError(ApplicationErrorType.UNKNOWN, "internal error"),
        ):
            with self.subTest(error=type(error).__name__):
                result = self._read(side_effect=error)
                self.assertEqual(FAIL, result.outcome)
                self.assertTrue(result.facts["reason"].startswith("rpc: "))
        with self.assertRaises(AttributeError):
            self._read(side_effect=AttributeError("harness bug"))

    def test_posix_errors_keep_their_code_out_of_errno(self) -> None:
        """A Thrift read keeps a POSIX code in `error_code`; a readdir records type and errno.

        The contracts matches a Thrift readdir error on (errorType, errno).
        """
        read = Action(
            "r1",
            "read",
            DEADLINE_OPERATION,
            payload={
                "path": REGISTERS[0],
                "channel": "thrift",
                "sync_timeout_seconds": 0,
            },
        )
        listing = Action(
            "d1",
            "readdir",
            DEADLINE_OPERATION,
            payload={"path": "tree/registers", "channel": "thrift"},
        )
        client = mock.Mock()
        client.getFileContent.return_value = GetFileContentResponse(
            blob=ScmBlobOrError(
                error=EdenError(
                    message="is a directory",
                    errorCode=errno.EISDIR,
                    errorType=EdenErrorType.POSIX_ERROR,
                )
            )
        )
        client.readdir.return_value = SimpleNamespace(
            dirLists=[
                DirListAttributeDataOrError(
                    error=EdenError(
                        message="not a directory",
                        errorCode=errno.ENOTDIR,
                        errorType=EdenErrorType.POSIX_ERROR,
                    )
                )
            ]
        )
        env = mock.MagicMock(spec=Environment)
        env.mount_path = "/mount"
        env.get_thrift_client.return_value.__enter__.return_value = client
        spec = _spec(REGISTERS[0], [read, listing], [("a", ["r1", "d1"])])
        backend = LiveBackend(env, spec)
        read_result, listing_result = backend(read), backend(listing)
        self.assertEqual(
            (FAIL, errno.EISDIR, False),
            (
                read_result.outcome,
                read_result.facts["error_code"],
                "errno" in read_result.facts,
            ),
        )
        self.assertEqual(
            BackendResult(
                FAIL,
                {
                    "reason": "thrift: not a directory",
                    "source": "thrift",
                    "error_type": "POSIX_ERROR",
                    "errno": errno.ENOTDIR,
                },
            ),
            listing_result,
        )


class ReadDigestBackendTest(unittest.TestCase):
    def test_read_records_digest(self) -> None:
        """Every read channel records the whole file as a 128-byte prefix, length, and sha256."""
        data = bytes(range(256)) + b"tail"
        payloads = {
            "r1": {"path": REGISTERS[0], "channel": "fuse"},
            "t1": {
                "path": REGISTERS[0],
                "channel": "thrift",
                "sync_timeout_seconds": 0,
            },
            "o1": {"path": REGISTERS[0]},
            "f1": {"fd": "o1"},
        }
        kinds = {"r1": "read", "t1": "read", "o1": "open", "f1": "read_fd"}
        actions = [
            Action(id, kinds[id], DEADLINE_OPERATION, payload=payloads[id])
            for id in ("o1", "r1", "t1", "f1")
        ]
        spec = _spec(REGISTERS[0], actions, [("a", [a.id for a in actions])])
        expected = {
            "bytes_prefix": data[:128].hex(),
            "length": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        }
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, REGISTERS[0]).parent.mkdir(parents=True)
            Path(tmp, REGISTERS[0]).write_bytes(data)
            env = mock.MagicMock(spec=Environment)
            env.mount_path = Path(tmp)
            client = env.get_thrift_client.return_value.__enter__.return_value
            client.getFileContent.return_value = GetFileContentResponse(
                blob=ScmBlobOrError(blob=data)
            )
            backend = LiveBackend(env, spec)
            self.assertEqual(OK, backend(actions[0]).outcome)
            for action in actions[1:]:
                with self.subTest(action=action.id):
                    result = backend(action)
                    self.assertEqual(
                        (OK, expected), (result.outcome, dict(result.facts))
                    )
