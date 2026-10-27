# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Live backend cases judged by `EDENFS_TARGET` (verdict v1).

Each case runs one backend fixture (`backends_fixtures`) against a real
EdenFS daemon, checks the recorded facts in the strict result
vocabulary, and requires the `EDENFS_TARGET` verdict: accept for green
runs, a reject on the control's rules for red ones. Live cases skip when
the fixture cannot start a daemon.
"""

from __future__ import annotations

import errno
import io
import os
import signal
import tempfile
import time
import unittest
from contextlib import redirect_stderr, redirect_stdout
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    check,
    REJECT,
    Verdict,
)
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    ExplorationResult,
    load_history,
    load_replay,
)
from eden.integration.correctness.exploratory.core.generate import compose
from eden.integration.correctness.exploratory.core.history import (
    Event,
    FAIL,
    InMemorySink,
    OK,
)
from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    DEADLINE_OPERATION,
    FINISHED,
    from_json,
    RunSpec,
    to_json,
)
from eden.integration.correctness.exploratory.core.scheduler import run_actions
from eden.integration.correctness.exploratory.core.target import RUN_DIR_ENV
from eden.integration.correctness.exploratory.edenfs import (
    environment as environment_module,
)
from eden.integration.correctness.exploratory.edenfs.backends import LiveBackend
from eden.integration.correctness.exploratory.edenfs.environment import (
    CHURN,
    Environment,
)
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    live_runner_main,
    run_live,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _artifact_directory,
    _dirstate_spec,
    _expected,
    _fail_on_setup_failure,
    _fd_spec,
    _fetch_tree_cancel_spec,
    _fetch_tree_status_spec,
    _forced_checkout_spec,
    _green_spec,
    _journal_window_spec,
    _kill_status_spec,
    _manifest_presence_spec,
    _mkdir_listing_spec,
    _mkdir_spec,
    _readdir_stream_spec,
    _red_spec,
    _rename_spec,
    _require_live,
    _SEED,
    _spec,
    _spec_env,
    _status_spec,
    _status_via_thrift_spec,
    _unlink_spec,
    _WIDE_ENTRIES,
    REGISTERS,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import WORKED_EXAMPLE


def _edenfs(spec: RunSpec) -> RunSpec:
    """Return `spec` parsed under `EDENFS_TARGET`."""
    return from_json(to_json(spec), EDENFS_TARGET)


def _problems(verdict: Verdict) -> list[tuple[str, str, str, str | None]]:
    """Return every obligation that is not satisfied, for failure messages."""
    report = verdict.report
    if report is None:
        return [("-", "-", verdict.reason, verdict.not_evaluated_cause)]
    return [
        (o.rule_id, o.scope, o.status.value, o.not_evaluated_reason or o.reason)
        for result in report.results
        for o in result.obligations
        if o.status is not ObligationStatus.SATISFIED
    ]


def _violated_rules(verdict: Verdict) -> set[str]:
    report = verdict.report
    assert report is not None, verdict.reason
    return {
        o.rule_id
        for result in report.results
        for o in result.obligations
        if o.status is ObligationStatus.VIOLATED
    }


def _completions(events: list[Event], *types: str) -> dict[str, Event]:
    kinds = types or (OK,)
    return {e.data["action"]: e for e in events if e.type in kinds}


def _status_codes(facts: dict[str, Any]) -> dict[str, str]:
    """Return a recorded status result's entries as {path: code}."""
    return {entry["path"]: entry["code"] for entry in facts["status"]}


def _judge(spec: RunSpec, events: list[Event]) -> Verdict:
    """Judge an in-process run (no containment, no execution document)."""
    return check(EDENFS_TARGET, _edenfs(spec), events)


class _ExplodingEnvironment:
    """Fake Environment whose setup fails after hypothetically starting."""

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        self.closed = False

    def setup(self) -> None:
        raise RuntimeError("daemon started but clone failed")

    def close(self) -> str | None:
        self.closed = True
        return None


class LiveRunnerSetupFailureTest(unittest.TestCase):
    def test_setup_failure_closes_environment(self) -> None:
        envs: list[_ExplodingEnvironment] = []
        real_environment = environment_module.Environment

        def _fake(*args: Any, **kwargs: Any) -> _ExplodingEnvironment:
            env = _ExplodingEnvironment(*args, **kwargs)
            envs.append(env)
            return env

        with tempfile.TemporaryDirectory() as tmp:
            spec_path = os.path.join(tmp, "run-spec.json")
            with open(spec_path, "w") as handle:
                handle.write(to_json(compose(EDENFS_TARGET, 0, _spec_env().target)))
            out, err = io.StringIO(), io.StringIO()
            with (
                mock.patch.dict(os.environ, {RUN_DIR_ENV: tmp}),
                mock.patch.object(environment_module, "Environment", _fake),
                redirect_stdout(out),
                redirect_stderr(err),
            ):
                code = live_runner_main(spec_path, None)
            self.assertEqual(code, 1)
            self.assertIn("setup-failed", err.getvalue())
            # The started-but-uncloned daemon is closed, not leaked.
            self.assertEqual([env.closed for env in envs], [True])
            self.assertIs(environment_module.Environment, real_environment)


class _LiveRunCase(unittest.TestCase):
    """Runs one spec through `run_live` (judged by `EDENFS_TARGET`)."""

    def _run(
        self, spec: RunSpec, prefix: str, red_control: str | None = None
    ) -> tuple[ExplorationResult, list[Event], str]:
        _require_live(self)
        with _artifact_directory(prefix) as tmp:
            result = run_live(spec, tmp, red_control=red_control)
            _fail_on_setup_failure(result, tmp)
            events = load_history(os.path.join(tmp, "history.jsonl"))
        return result, events, tmp

    def assertAccepted(self, result: ExplorationResult) -> None:
        self.assertEqual(
            (result.execution.status, result.execution.reason), (COMPLETED, "ok")
        )
        self.assertEqual(
            ACCEPT,
            result.verdict.kind,
            (result.verdict.reason, _problems(result.verdict)),
        )


class ReaddirStreamLiveTest(_LiveRunCase):
    def test_readdir_stream_pauses_and_resumes(self) -> None:
        """The child pauses after pause_after entries; resume drains every wide entry once."""
        result, events, tmp = self._run(_readdir_stream_spec(), "readdir-stream.")
        completions = _completions(events)
        self.assertEqual(_WIDE_ENTRIES // 2, completions["d0"].data["paused_at"])
        entries = completions["d1"].data["entries"]
        self.assertEqual(_WIDE_ENTRIES, len(entries))
        self.assertEqual(_WIDE_ENTRIES, len(set(entries)))
        self.assertTrue(all(len(name) >= 64 for name in entries))
        self.assertLess(
            completions["d0"].time,
            next(e for e in events if e.data.get("action") == "d1").time,
        )
        positions = (
            completions["d0"].data["pos_at_pause"],
            completions["d1"].data["pos_at_end"],
        )
        self.assertGreater(positions[1], positions[0], positions)
        self.assertAccepted(result)
        print(f"readdir stream artifacts: {tmp} positions: {positions}")


class CheckoutParkTest(_LiveRunCase):
    def test_checkout_begin_park_witness_release(self) -> None:
        """The parked `sl goto` resumes after release and refuses the dirtied register.

        At D124075931 `c0` succeeded: register images were revision-stable.
        With revision-specific images, `w1` (acked while `c0` is parked
        at checkout-begin) dirties r0, which revision 2 changes, so the
        resumed update aborts (Sapling's update aborts or succeeds as a whole).
        """
        spec = from_json(WORKED_EXAMPLE, EDENFS_TARGET)
        result, events, _tmp = self._run(spec, "checkout-park.")
        invokes = {e.data["action"]: e for e in events if e.type == "invoke"}
        done = _completions(events, OK, FAIL)
        self.assertLess(invokes["c0"].time, done["h1"].time)
        self.assertTrue(done["h1"].data["hit"])
        self.assertLess(done["w1"].time, done["x1"].time)
        self.assertLess(done["x1"].time, done["c0"].time)
        self.assertEqual(FAIL, done["c0"].type, done["c0"].data)
        self.assertIn("tree/registers/r0.reg", done["c0"].data["stderr"])
        self.assertAccepted(result)


class LiveBackendTest(_LiveRunCase):
    """In-process LiveBackend runs and `run_live` runs of the backend fixtures."""

    def setUp(self) -> None:
        super().setUp()
        _require_live(self)
        self.env = Environment(seed=_SEED, registers=REGISTERS[:1], sink=InMemorySink())
        self.addCleanup(self.env.close)
        self.env.setup()
        self.register = REGISTERS[0]

    def _in_process(self, spec: RunSpec, red_control: str | None = None) -> list[Event]:
        sink = InMemorySink()
        status = run_actions(
            spec,
            LiveBackend(self.env, spec, red_control=red_control),
            sink,
            time.monotonic,
        )
        self.assertEqual((status.status, status.reason), (COMPLETED, None))
        return list(sink.events)

    def test_green_two_actor_run_is_accepted(self) -> None:
        spec = _green_spec(self.register)
        events = self._in_process(spec)
        completions = [e for e in events if e.type == OK]
        self.assertEqual(len(completions), len(spec.actions))
        by_action = {e.data["action"]: e for e in completions}
        # Both sync modes complete and observe the single written value.
        for action_id in ("r1", "t0", "t60"):
            self.assertEqual(
                by_action[action_id].data["bytes_prefix"], _expected("a", "green-w1")
            )
        verdict = _judge(spec, events)
        self.assertEqual(ACCEPT, verdict.kind, (verdict.reason, _problems(verdict)))

    def test_red_torn_write_is_rejected(self) -> None:
        spec = _red_spec(self.register)
        verdict = _judge(spec, self._in_process(spec, red_control="torn-write"))
        self.assertEqual(REJECT, verdict.kind)
        self.assertEqual({"bytes.torn-read"}, _violated_rules(verdict))

    def test_green_status_probe_is_accepted(self) -> None:
        spec = _status_spec(self.register)
        events = self._in_process(spec)
        by_action = _completions(events)
        self.assertEqual("M", _status_codes(by_action["s1"].data)[self.register])
        verdict = _judge(spec, events)
        self.assertEqual(ACCEPT, verdict.kind, (verdict.reason, _problems(verdict)))

    def test_green_status_via_thrift_controls_are_accepted(self) -> None:
        """Thrift status at the explicit and the current parent both list the write.

        The old checker judged only sl status, so this run was not
        evaluated; `status.thrift-model-stale-status` judges both results now.
        """
        result, events, tmp = self._run(
            _status_via_thrift_spec(self.register), "thrift-status."
        )
        completions = _completions(events)
        self.assertEqual(set(completions), {"w1", "s1", "s2"})
        expected = {self.register: "MODIFIED"}
        self.assertEqual(expected, _status_codes(completions["s1"].data))
        self.assertEqual(expected, _status_codes(completions["s2"].data))
        self.assertEqual(completions["s1"].data["parent"], self.env.commits[3].node)
        self.assertEqual(
            completions["s2"].data["parent"], completions["s1"].data["parent"]
        )
        self.assertAccepted(result)
        print(f"thrift status artifacts: {tmp}")

    def test_manifest_presence_after_checkout(self) -> None:
        """A Thrift checkout leaves deleted paths absent and added paths present."""
        result, events, tmp = self._run(
            _manifest_presence_spec(self.register), "manifest-presence."
        )
        completions = _completions(events)
        manifest = completions["m1"].data
        self.assertTrue(manifest["deleted_presence"])
        self.assertTrue(manifest["added_presence"])
        self.assertFalse(any(item["present"] for item in manifest["deleted_presence"]))
        self.assertTrue(all(item["present"] for item in manifest["added_presence"]))
        self.assertEqual(completions["c1"].data["via"], "thrift")
        self.assertNotIn("timing", completions["c1"].data)
        self.assertAccepted(result)
        print(f"manifest presence artifacts: {tmp}")

    def test_force_reports_conflicts_and_applies(self) -> None:
        """FORCE reports the dirty register as a conflict and restores its committed bytes.

        TreeInode skips unchanged entries only when not forcing; under FORCE
        it records the conflict and replaces the entry, so the read sees
        the destination's committed register and the checker accepts.
        """
        spec = _forced_checkout_spec(self.register)
        events = self._in_process(spec)
        by_action = _completions(events)
        self.assertEqual(set(by_action), {"w1", "c0", "r1"})
        self.assertEqual(by_action["c0"].data["via"], "thrift")
        self.assertGreaterEqual(by_action["c0"].data["conflicts"], 1)
        verdict = _judge(spec, events)
        print(
            f"forced checkout conflicts={by_action['c0'].data['conflicts']}"
            f" {verdict.kind} {verdict.reason}"
        )
        self.assertEqual(ACCEPT, verdict.kind, (verdict.reason, _problems(verdict)))

    def test_open_read_close_live(self) -> None:
        """A held fd observes a later by-path write to the same file (same-fd coherence)."""
        spec = _fd_spec(self.register)
        events = self._in_process(spec)
        by_action = _completions(events)
        self.assertEqual(set(by_action), {"o0", "w1", "f1", "x1"})
        self.assertEqual(by_action["f1"].data["bytes_prefix"], _expected("b", "fd-w1"))
        verdict = _judge(spec, events)
        self.assertEqual(ACCEPT, verdict.kind, (verdict.reason, _problems(verdict)))

    def test_rename_moves_bytes_live(self) -> None:
        """After rename(2) the source is ENOENT and the destination holds the acked bytes."""
        result, events, _tmp = self._run(_rename_spec(self.register), "rename.")
        by_action = _completions(events, OK, FAIL)
        self.assertEqual(by_action["n0"].type, OK)
        self.assertEqual(
            (FAIL, "ENOENT"),
            (by_action["ra"].type, errno.errorcode[by_action["ra"].data["errno"]]),
        )
        self.assertEqual(
            by_action["rb"].data["bytes_prefix"], _expected("a", "rename-w1")
        )
        self.assertAccepted(result)

    def test_mkdir_is_a_directory_on_both_channels_live(self) -> None:
        """mkdir yields a directory over FUSE and Thrift."""
        result, events, _tmp = self._run(_mkdir_spec(self.register), "mkdir.")
        by_action = _completions(events)
        self.assertEqual(
            ("directory", "directory"),
            (by_action["g0"].data["fuse_type"], by_action["g0"].data["thrift_type"]),
        )
        self.assertAccepted(result)

    def test_unlink_shows_missing_in_status_live(self) -> None:
        """An unlinked committed register is reported `!` (missing) by sl status."""
        result, events, _tmp = self._run(_unlink_spec(self.register), "unlink.")
        by_action = _completions(events)
        self.assertEqual("!", _status_codes(by_action["s1"].data).get(self.register))
        self.assertAccepted(result)

    def test_sl_rm_shows_removed_in_status_live(self) -> None:
        """A register removed through sl is reported `R` by sl status."""
        spec = _dirstate_spec(self.register, "sl_rm", {"path": self.register})
        result, events, _tmp = self._run(spec, "sl-rm.")
        by_action = _completions(events)
        self.assertEqual({self.register: "R"}, _status_codes(by_action["s1"].data))
        self.assertAccepted(result)

    def test_sl_mv_shows_removed_and_added_in_status_live(self) -> None:
        """A register moved through sl is `R` and its destination `A` in sl status."""
        moved = "tree/registers/moved.reg"
        payload = {"source": self.register, "destination": moved}
        result, events, _tmp = self._run(
            _dirstate_spec(self.register, "sl_mv", payload), "sl-mv."
        )
        by_action = _completions(events)
        self.assertEqual(
            {self.register: "R", moved: "A"}, _status_codes(by_action["s1"].data)
        )
        self.assertAccepted(result)

    def test_mkdir_listed_by_parent_live(self) -> None:
        """The wide dir's readdir lists a made directory exactly once."""
        result, events, tmp = self._run(_mkdir_listing_spec(), "mkdir-listing.")
        by_action = _completions(events)
        entries = by_action["d1"].data["entries"]
        self.assertEqual(1, entries.count("made"))
        self.assertEqual(_WIDE_ENTRIES + 1, len(entries))
        self.assertAccepted(result)
        print(f"mkdir artifacts: {tmp}")

    def test_journal_window_attributes_write(self) -> None:
        """A journal observation attributes its advance to the preceding write."""
        result, events, tmp = self._run(
            _journal_window_spec(self.register), "journal-window."
        )
        journal = _completions(events)["j1"].data
        self.assertGreater(journal["sequence_number"], 0)
        changed = {item["path"]: item for item in journal["changed_paths"]}
        self.assertTrue(changed[self.register]["existed_before"])
        self.assertTrue(changed[self.register]["existed_after"])
        self.assertAccepted(result)
        print(f"journal window artifacts: {tmp}")

    def test_fetch_tree_status_current_parent_warm_and_cold(self) -> None:
        """Characterize cold reachability and the warm-cache checkpoint miss."""
        for cache, revision in (("cold", 3), ("warm", None)):
            with self.subTest(cache=cache, revision=revision):
                spec = _fetch_tree_status_spec(
                    self.register, warm=cache == "warm", revision=revision
                )
                result, events, tmp = self._run(spec, f"fetch-tree-{cache}.")
                invokes = {e.data["action"]: e for e in events if e.type == "invoke"}
                completions = _completions(events, OK, FAIL)
                expected = {self.register: "MODIFIED"}
                cleanup = [e for e in events if e.type == "cleanup-removed"]
                if cache == "warm":
                    self.assertEqual(result.execution.status, "harness-failure")
                    self.assertIn("fault-witness-failed", result.execution.reason)
                    self.assertEqual(expected, _status_codes(completions["p0"].data))
                    self.assertEqual(expected, _status_codes(completions["s1"].data))
                    self.assertNotIn("h1", completions)
                    self.assertEqual(1, len(cleanup))
                    self.assertEqual(
                        (True, False),
                        (cleanup[0].data["miss"], cleanup[0].data["leak"]),
                    )
                    print(f"fetch-tree status cache={cache} reachability=miss {tmp}")
                    continue
                self.assertEqual(OK, completions["s1"].type)
                self.assertEqual(expected, _status_codes(completions["s1"].data))
                self.assertEqual(expected, _status_codes(completions["s2"].data))
                self.assertEqual(
                    completions["h1"].data["witness"],
                    completions["s1"].data["parent"],
                )
                self.assertEqual(
                    completions["s2"].data["parent"], completions["s1"].data["parent"]
                )
                self.assertLess(invokes["s1"].time, completions["h1"].time)
                self.assertLess(invokes["x1"].time, completions["s1"].time)
                self.assertEqual(1, len(cleanup))
                self.assertEqual(
                    (False, False), (cleanup[0].data["miss"], cleanup[0].data["leak"])
                )
                self.assertEqual("backing", spec.environment.target["fixture_source"])
                self.assertAccepted(result)
                print(f"fetch-tree status cache={cache} reachability=hit {tmp}")

    def test_fetch_tree_status_noncurrent_parent_is_a_typed_failure(self) -> None:
        """A Thrift status of a non-current parent fails typed `OUT_OF_DATE_PARENT`.

        D124075931 raised a harness failure here; the backend records the EdenError
        as a typed `fail` (`source`, `error_type`), which `status` declares
        a state-dependent failure.
        """
        action = Action(
            id="s1",
            kind="status",
            deadline=DEADLINE_OPERATION,
            payload={"channel": "thrift", "revision": 2},
        )
        spec = _spec(self.register, [action], [("worker", [action.id])])
        result = LiveBackend(self.env, spec)(action)
        print(f"noncurrent status: {result}")
        self.assertEqual(FAIL, result.outcome)
        self.assertEqual(
            ("thrift", "OUT_OF_DATE_PARENT"),
            (result.facts.get("source"), result.facts.get("error_type")),
        )
        self.assertNotEqual(self.env.commits[2].node, self.env.commits[3].node)

    def test_fetch_tree_native_revision_kill_drains_before_current_probe(self) -> None:
        """Prove native current-p1 cancellation, drain, and later status."""
        result, events, tmp = self._run(
            _fetch_tree_cancel_spec(self.register), "fetch-tree-cancel."
        )
        invokes = {e.data["action"]: e for e in events if e.type == "invoke"}
        completions = _completions(events, OK, FAIL)
        self.assertEqual(result.execution.status, COMPLETED)
        self.assertEqual(FAIL, completions["s1"].type)
        self.assertEqual(-signal.SIGKILL, completions["s1"].data["returncode"])
        self.assertNotIn("status", completions["s1"].data)
        self.assertEqual(OK, completions["k1"].type)
        self.assertTrue(completions["k1"].data["group_empty"])
        with self.assertRaises(ProcessLookupError):
            os.killpg(completions["k1"].data["pgid"], 0)
        self.assertLess(invokes["s1"].time, completions["h1"].time)
        self.assertLess(completions["h1"].time, invokes["k1"].time)
        self.assertLess(completions["k1"].time, invokes["x1"].time)
        self.assertLess(completions["x1"].time, invokes["s2"].time)
        self.assertEqual(completions["h1"].data["witness"], self.env.commits[3].node)
        self.assertNotEqual(completions["h1"].data["witness"], self.env.commits[2].node)
        cleanup = [e for e in events if e.type == "cleanup-removed"]
        self.assertEqual(1, len(cleanup))
        self.assertEqual(
            (False, False), (cleanup[0].data["miss"], cleanup[0].data["leak"])
        )
        self.assertAccepted(result)
        print(f"fetch-tree native kill artifacts: {tmp}")

    def test_kill_process_empties_group(self) -> None:
        result, events, _tmp = self._run(
            _kill_status_spec(self.register), "kill-status."
        )
        completions = _completions(events, OK, FAIL)
        self.assertEqual(FAIL, completions["s1"].type)
        self.assertEqual(-signal.SIGKILL, completions["s1"].data["returncode"])
        self.assertNotIn("status", completions["s1"].data)
        self.assertEqual(OK, completions["k1"].type)
        self.assertEqual("s1", completions["k1"].data["target_action"])
        self.assertTrue(completions["k1"].data["group_empty"])
        with self.assertRaises(ProcessLookupError):
            os.killpg(completions["k1"].data["pgid"], 0)
        # The killed probe has no status to judge; the rest of the run is.
        self.assertAccepted(result)

    def test_green_churn_status_probe_is_accepted(self) -> None:
        # A write dirties a tracked churn file through the same pwrite
        # path; the probe reports it M and the checker accepts.
        churn = CHURN[0]
        u1 = Action(
            id="u1",
            kind="write",
            deadline=DEADLINE_OPERATION,
            payload={"path": churn, "value_id": "churn-u1"},
        )
        s1 = Action(
            id="s1",
            kind="status",
            deadline=DEADLINE_OPERATION,
            requires=((u1.id, FINISHED),),
            payload={"channel": "sl"},
        )
        spec = _spec(self.register, [u1, s1], [("a", ["u1", "s1"])])
        events = self._in_process(spec)
        self.assertEqual("M", _status_codes(_completions(events)["s1"].data)[churn])
        verdict = _judge(spec, events)
        self.assertEqual(ACCEPT, verdict.kind, (verdict.reason, _problems(verdict)))

    def test_red_status_loss_is_rejected(self) -> None:
        spec = _status_spec(self.register)
        verdict = _judge(spec, self._in_process(spec, red_control="status-loss"))
        self.assertEqual(REJECT, verdict.kind)
        self.assertIn("status.status-diff-mismatch", _violated_rules(verdict))

    def test_red_diff_loss_is_rejected(self) -> None:
        # Status tells the truth but diff_paths drops the modification:
        # the status/diff agreement rejects the disagreement.
        spec = _status_spec(self.register)
        verdict = _judge(spec, self._in_process(spec, red_control="diff-loss"))
        self.assertEqual(REJECT, verdict.kind)
        self.assertIn("status.status-diff-mismatch", _violated_rules(verdict))

    def test_green_probe_reports_diff_paths(self) -> None:
        # The live probe's diff fact names the written register, so the
        # mismatch rule has a true observation to check.
        spec = _status_spec(self.register)
        by_action = _completions(self._in_process(spec))
        self.assertEqual([self.register], by_action["s1"].data["diff_paths"])

    def test_run_live_writes_complete_artifacts(self) -> None:
        spec = _green_spec(self.register)
        with tempfile.TemporaryDirectory() as tmp:
            result = run_live(spec, tmp)
            _fail_on_setup_failure(result, tmp)
            self.assertAccepted(result)
            for name in (
                "run-spec.json",
                "history.jsonl",
                "execution.json",
                "verdict.json",
                "replay.json",
            ):
                self.assertTrue(os.path.isfile(os.path.join(tmp, name)), name)
            events = load_history(os.path.join(tmp, "history.jsonl"))
            # One invoke plus one completion per action, the target-setup and
            # target-closed events, and the cleanup event.
            self.assertEqual(len(events), 2 * len(spec.actions) + 3)
            target_events = [
                e.type for e in events if e.type in ("target-setup", "target-closed")
            ]
            self.assertEqual(["target-setup", "target-closed"], target_events)
            replay_spec, replay_events = load_replay(
                os.path.join(tmp, "replay.json"), EDENFS_TARGET
            )
            self.assertEqual(len(replay_events), len(events))
            offline = check(EDENFS_TARGET, replay_spec, replay_events)
            self.assertEqual(result.verdict.kind, offline.kind)
