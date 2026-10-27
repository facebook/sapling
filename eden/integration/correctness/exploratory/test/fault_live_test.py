# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Live tests for the fault catalog: green park-and-heal, witness failure, and
process-group cleanup against a real daemon. They skip only when the host
has no FUSE device or user DBus session; a setup failure after that check
fails.
Run as a script with --checkpoint-fixture or --release-at-remove, this
module is the runner fixture those tests spawn."""

import os
import re
import signal
import sys
import threading
import time
import unittest
from dataclasses import replace
from functools import partial
from pathlib import Path
from typing import Any, cast
from unittest import mock

from eden.fs.service.eden.thrift_types import TimeSpec
from eden.integration.correctness.exploratory.cli import (
    _campaign_provenance,
    _run_campaign_seed,
)
from eden.integration.correctness.exploratory.core.campaign import KEEP_ALL
from eden.integration.correctness.exploratory.core.checker import REJECT
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    ExplorationResult,
    HARNESS_FAILURE,
    load_history,
    run_exploration,
    runner_argv_for,
    runner_main,
)
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    CLEANUP_REMOVED,
    FAIL,
    HistorySink,
    InMemorySink,
    INVOKE,
    OK,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    DEADLINE_OPERATION,
    FINISHED,
    RunSpec,
    STARTED,
)
from eden.integration.correctness.exploratory.core.scheduler import (
    FAULT_WITNESS_FAILED,
    HarnessFailure,
)
from eden.integration.correctness.exploratory.edenfs import (
    environment as edenfs_environment,
)
from eden.integration.correctness.exploratory.edenfs.actions import repository
from eden.integration.correctness.exploratory.edenfs.backends import LiveBackend
from eden.integration.correctness.exploratory.edenfs.environment import Environment
from eden.integration.correctness.exploratory.edenfs.properties.status import (
    MODEL_LIVE,
    SL_FUSE,
    SL_MODEL,
)
from eden.integration.correctness.exploratory.edenfs.sites import LOOKUP_SITE, SITES
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    run_live,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _artifact_directory,
    _fail_on_setup_failure,
    _require_live,
    _stderr_log,
)
from eden.integration.correctness.exploratory.schemas import load_admission, RunPurpose
from eden.integration.correctness.exploratory.test.fault_fixtures import (
    _assert_accept,
    _await,
    _behavior_spec,
    _checkout,
    _checkpoint_spec,
    _committed,
    _green_spec,
    _install,
    _SEED,
    _spec,
    _status_code,
    _status_guard_spec,
    _tree_checkout_ordinal_spec,
    _unsatisfied,
    _violated_rules,
    _wait_live_ready,
    _wait_marker,
    _wait_sl_child,
    _wfpw_spec,
    DIRT,
    REGISTERS,
    validate,
)
from eden.integration.lib import edenclient

# The stale-status rules a missing expected modification violates: the only
# rejects the honest reproductions admit.
_STALE_STATUS_RULES = frozenset({MODEL_LIVE, SL_MODEL, SL_FUSE})


class _CheckpointEnvironment(Environment):
    """Prepare a true inode reload using existing debug RPCs only in tests."""

    def __init__(self, *args: Any, site: str, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self._site = site

    def setup(self) -> None:
        """Keep the target's FUSE dentry while unloading its daemon inode."""
        factory = partial(
            edenclient.EdenFS,
            logging_settings={"eden.fs.fuse.FuseRequestContext": "DBG5"},
        )
        with mock.patch.object(edenclient, "EdenFS", factory):
            super().setup()
        path = REGISTERS[0]
        inode = (self.mount_path / path).stat().st_ino
        if self._site == "inodemap-load":
            mount = os.fsencode(self.mount_path)
            with self.get_thrift_client() as thrift_client:
                client = cast(Any, thrift_client)
                client.invalidateKernelInodeCache(mount, os.fsencode(path))
                deadline = time.monotonic() + 60
                while True:
                    client.unloadInodeForPath(
                        mount, os.fsencode(Path(path).parent), TimeSpec()
                    )
                    info = client.debugGetInodePath(mount, inode)
                    if not info.loaded:
                        break
                    if time.monotonic() >= deadline:
                        raise RuntimeError("checkpoint fixture inode stayed loaded")
                    time.sleep(0.01)
                if not info.linked or os.fsdecode(info.path) != path:
                    raise RuntimeError("checkpoint fixture lost the target inode")
            print(f"checkpoint-unloaded={inode}", file=sys.stderr, flush=True)
        key = path if self._site == "journal-record" else str(inode)
        print(f"checkpoint-key={key}", file=sys.stderr, flush=True)


def _checkpoint_environment(spec: RunSpec, sink: HistorySink) -> Environment:
    """Build the target's own environment as the test-only checkpoint fixture."""
    site = spec.action("f1").payload["site"]
    with mock.patch.object(
        edenfs_environment,
        "Environment",
        partial(_CheckpointEnvironment, site=site),
    ):
        return cast(Environment, EDENFS_TARGET.make_environment(spec, sink))


def _checkpoint_runner_main(spec_path: str) -> int:
    """Use the ordinary runner with a test-only inode-state fixture."""
    target = replace(EDENFS_TARGET, make_environment=_checkpoint_environment)
    return runner_main(spec_path, target)


class _ReleaseAtFaultRemoveBackend:
    """Hold fault removal until the supervisor releases the parked checkout."""

    def __init__(self, backend: LiveBackend, ready: str, release: str) -> None:
        self._backend = backend
        self._ready = ready
        self._release = release

    def __call__(self, action: Action) -> Any:
        if action.kind == "fault_remove":
            with open(self._ready, "w") as handle:
                handle.write(f"{action.id}\n")
            while not os.path.isfile(self._release):
                time.sleep(0.05)
        return self._backend(action)

    def unwind(self, sink: Any, clock: Any) -> dict[str, Any]:
        return self._backend.unwind(sink, clock)

    def __getattr__(self, name: str) -> Any:
        # Staged cleanup (`unwind_faults`, `close_resources`) reaches the real backend.
        return getattr(self._backend, name)


def _release_runner_main(spec_path: str, ready: str, release: str) -> int:
    """Run the real live backend with the test-only fault-remove release gate."""

    def make_backend(environment: Any, spec: RunSpec, red_control: str | None) -> Any:
        return _ReleaseAtFaultRemoveBackend(
            LiveBackend(environment, spec, red_control), ready, release
        )

    target = replace(EDENFS_TARGET, make_backend=make_backend)
    return runner_main(spec_path, target)


def _argument_value(argv: list[str], name: str) -> str:
    """Return the value following one required helper-runner argument."""
    try:
        return argv[argv.index(name) + 1]
    except (ValueError, IndexError) as error:
        raise ValueError(f"missing {name}") from error


class LiveFaultTest(unittest.TestCase):
    def setUp(self) -> None:
        _require_live(self)

    def _check_internal_checkpoint(self, site: str, behavior: str) -> None:
        """Verify exact park/release or consumed error with raw daemon evidence."""
        spec = _checkpoint_spec(site, behavior)
        validate(spec)
        with _artifact_directory(f"fault-{site}-{behavior}-") as tmp:
            argv = runner_argv_for(os.path.join(tmp, "run-spec.json"), "edenfs")
            argv[1] = os.path.abspath(__file__)
            argv.append("--checkpoint-fixture")
            result = run_exploration(spec, tmp, target=EDENFS_TARGET, runner_argv=argv)
            stderr = _stderr_log(tmp)
            self.assertNotIn("checkpoint fixture", stderr)
            _fail_on_setup_failure(result, tmp)
            self.assertEqual(result.execution.status, COMPLETED, result.execution)
            _assert_accept(self, result.verdict)
            key_lines = [
                line.removeprefix("checkpoint-key=")
                for line in stderr.splitlines()
                if line.startswith("checkpoint-key=")
            ]
            self.assertEqual(len(key_lines), 1, stderr)
            key = key_lines[0]
            if site == "inodemap-load":
                self.assertIn(f"checkpoint-unloaded={key}", stderr)
            events = load_history(os.path.join(tmp, "history.jsonl"))
            invokes = {e.data["action"]: e for e in events if e.type == INVOKE}
            completions = {e.data["action"]: e for e in events if e.type in {OK, FAIL}}
            self.assertEqual(completions["f1"].data["selector"], re.escape(key))
            if site == "inodemap-load":
                unload = completions["u0"]
                self.assertEqual(unload.type, OK)
                self.assertGreaterEqual(unload.data["prepared"], 1)
                self.assertIn("num_invalidated", unload.data)
                self.assertLess(unload.time, invokes["f1"].time)
            target = completions["t1"]
            if behavior == "block":
                self.assertEqual(completions["h1"].data["witness"], key)
                self.assertTrue(completions["h1"].data["hit"])
                self.assertLess(invokes["t1"].time, completions["h1"].time)
                self.assertLess(invokes["x1"].time, target.time)
                self.assertEqual(target.type, OK)
            else:
                self.assertEqual(target.type, FAIL)
                self.assertIn("Input/output error", target.data["reason"])
                self.assertNotIn("exploratory fault F", target.data["reason"])
                self.assertIn(f"error fault hit: {site}, {key}", stderr)
                self.assertIn("exploratory fault F", stderr)
            self.assertEqual(completions["x1"].data["removed"], 0)
            cleanups = [e for e in events if e.type == CLEANUP_REMOVED]
            self.assertEqual(len(cleanups), 1)
            self.assertEqual(
                (cleanups[0].data["miss"], cleanups[0].data["leak"]),
                (False, False),
                cleanups[0].data,
            )
            self.assertEqual(
                sorted(os.listdir(tmp)),
                [
                    "execution.json",
                    "history.jsonl",
                    "replay.json",
                    "run-spec.json",
                    "stderr.log",
                    "verdict.json",
                ],
            )
            print(
                f"checkpoint={site} behavior={behavior} key={key} "
                f"target={target.type} artifacts={tmp}"
            )

    def test_overlay_write_block_witness(self) -> None:
        """Overlay pwrite parks on the target inode and resumes after release."""
        self._check_internal_checkpoint("overlay-write", "block")

    def test_overlay_write_error_witness(self) -> None:
        """Overlay failure reaches FUSE as EIO and retains the daemon message."""
        self._check_internal_checkpoint("overlay-write", "error")

    def test_inodemap_load_block_witness(self) -> None:
        """An unloaded referenced inode parks during numeric FUSE lookup."""
        self._check_internal_checkpoint("inodemap-load", "block")

    def test_inodemap_load_error_witness(self) -> None:
        """An inode load failure completes its FUSE waiter with EIO."""
        self._check_internal_checkpoint("inodemap-load", "error")

    def test_journal_record_block_witness(self) -> None:
        """Journal append parks on the written relative path before its ack."""
        self._check_internal_checkpoint("journal-record", "block")

    def test_journal_record_error_witness(self) -> None:
        """A journal error fails the write even though its bytes may have landed."""
        self._check_internal_checkpoint("journal-record", "error")

    def test_tree_checkout_ordinal_parks_under_key(self) -> None:
        """Ordinal three parks at the third checkout directory below the key."""
        spec = _tree_checkout_ordinal_spec(3)
        validate(spec)
        with _artifact_directory("tree-checkout-hit-") as tmp:
            result = run_live(spec, tmp)
            _fail_on_setup_failure(result, tmp)
            events = load_history(os.path.join(tmp, "history.jsonl"))
            completions = {
                event.data["action"]: event
                for event in events
                if event.type in {OK, FAIL}
            }
            self.assertEqual(result.execution.status, COMPLETED)
            _assert_accept(self, result.verdict)
            self.assertTrue(completions["h1"].data["hit"])
            self.assertTrue(completions["h1"].data["witness"].startswith("tree/a1/"))
            self.assertEqual(completions["f1"].data["ordinal"], 3)
            cleanups = [event for event in events if event.type == CLEANUP_REMOVED]
            self.assertEqual(len(cleanups), 1)
            self.assertEqual(
                (cleanups[0].data["miss"], cleanups[0].data["leak"]),
                (False, False),
                cleanups[0].data,
            )
            print(
                f"tree-checkout ordinal=3 witness="
                f"{completions['h1'].data['witness']} artifacts={tmp}"
            )

    def test_tree_checkout_ordinal_overshoot_misses(self) -> None:
        """An ordinal beyond the checkout traversal fails and drains cleanly."""
        spec = _tree_checkout_ordinal_spec(99)
        validate(spec)
        with _artifact_directory("tree-checkout-overshoot-") as tmp:
            result = run_live(spec, tmp)
            _fail_on_setup_failure(result, tmp)
            self.assertEqual(result.execution.status, HARNESS_FAILURE)
            events = load_history(os.path.join(tmp, "history.jsonl"))
            cleanups = [event for event in events if event.type == CLEANUP_REMOVED]
            self.assertEqual(len(cleanups), 1)
            self.assertFalse(cleanups[0].data["hit"], cleanups[0].data)
            self.assertEqual(
                (cleanups[0].data["miss"], cleanups[0].data["leak"]),
                (True, False),
                cleanups[0].data,
            )
            self.assertGreaterEqual(
                cleanups[0].data.get("removed", 0), 1, cleanups[0].data
            )
            print(f"tree-checkout ordinal=99 miss artifacts={tmp}")

    def test_public_master_error_and_delay_behaviors(self) -> None:
        """Each public-master site reports an injected error and real delay."""
        for site, site_def in SITES.items():
            if site_def.daemon != "public-master":
                continue
            for behavior in ("error", "delay"):
                if behavior not in site_def.behaviors:
                    continue
                with self.subTest(site=site, behavior=behavior):
                    spec = _behavior_spec(REGISTERS[1], site, behavior)
                    with _artifact_directory(f"fault-{site}-{behavior}-") as tmp:
                        result = run_live(spec, tmp)
                        _fail_on_setup_failure(result, tmp)
                        self.assertEqual(
                            result.execution.status, COMPLETED, result.execution
                        )
                        events = load_history(os.path.join(tmp, "history.jsonl"))
                        invokes = {
                            event.data["action"]: event
                            for event in events
                            if event.type == INVOKE
                        }
                        completions = {
                            event.data["action"]: event
                            for event in events
                            if event.type in {OK, FAIL}
                        }
                        target = completions["t1"]
                        if behavior == "error":
                            self.assertEqual(target.type, FAIL)
                            if site == LOOKUP_SITE:
                                self.assertIn(
                                    "Input/output error", target.data["reason"]
                                )
                                self.assertNotIn(
                                    "exploratory fault F", target.data["reason"]
                                )
                                self.assertIn(
                                    "error fault hit: FuseDispatcherImpl::lookup, r1.reg",
                                    _stderr_log(tmp),
                                )
                            else:
                                self.assertIn(
                                    "exploratory fault F", target.data["reason"]
                                )
                        else:
                            self.assertEqual(target.type, OK)
                            self.assertGreaterEqual(
                                target.time - invokes["t1"].time,
                                spec.action("f1").payload["delay_ms"] / 1000,
                            )
                        self.assertEqual(completions["x1"].data.get("removed"), 0)
                        _assert_accept(self, result.verdict)

    def test_green_worked_example(self) -> None:
        """Fault hit lands after checkout invoke; checkout heals cleanly."""
        spec = _green_spec(REGISTERS[0])
        with _artifact_directory("fault-green-") as tmp:
            result = run_live(spec, tmp)
            _fail_on_setup_failure(result, tmp)
            self.assertEqual(
                result.execution.status,
                COMPLETED,
                result.execution,
            )
            self.assertEqual(result.execution.reason, "ok")
            _assert_accept(self, result.verdict)
            events = load_history(os.path.join(tmp, "history.jsonl"))
            invokes = {e.data["action"]: e for e in events if e.type == INVOKE}
            oks = {e.data["action"]: e for e in events if e.type == OK}
            # The witness fires while parked; checkout ends after the heal releases it.
            # No order is asserted between the remover's return and the
            # target: the checkout can complete on the unblock before
            # remove returns. The causal order is remove-invoked first.
            self.assertLess(invokes["c0"].time, oks["h1"].time)
            self.assertLess(invokes["x1"].time, oks["c0"].time)
            # w1 runs only after the witness: the validated requires edge.
            self.assertIn(("h1", FINISHED), spec.action("w1").requires)
            self.assertEqual(oks["h1"].data.get("hit"), True)
            # The parked checkout has not changed the tree: both channels
            # still serve the start revision's register image.
            start = repository(spec)["commit_count"] - 1
            for action_id in ("r1", "t1"):
                self.assertEqual(
                    oks[action_id].data.get("bytes_prefix"),
                    _committed(spec, REGISTERS[0], start),
                )
            cleanups = [e for e in events if e.type == CLEANUP_REMOVED]
            self.assertEqual(len(cleanups), 1)
            self.assertEqual(
                (cleanups[0].data.get("miss"), cleanups[0].data.get("leak")),
                (False, False),
                cleanups[0].data,
            )

    def test_green_status_guard(self) -> None:
        """Parked status fill heals across checkout; both probes see M."""
        spec = _status_guard_spec(REGISTERS[0])
        with _artifact_directory("fault-status-guard-") as tmp:
            result = run_live(spec, tmp)
            _fail_on_setup_failure(result, tmp)
            self.assertEqual(
                result.execution.status,
                COMPLETED,
                result.execution,
            )
            self.assertEqual(result.execution.reason, "ok")
            _assert_accept(self, result.verdict)
            events = load_history(os.path.join(tmp, "history.jsonl"))
            invokes = {e.data["action"]: e for e in events if e.type == INVOKE}
            oks = {e.data["action"]: e for e in events if e.type == OK}
            # The witness fires while parked; the checkout heals across
            # the park (its completion precedes the probe's). No order is
            # asserted between the remover and the probe.
            self.assertLess(invokes["s1"].time, oks["h1"].time)
            self.assertLess(oks["c0"].time, oks["s1"].time)
            self.assertEqual(oks["h1"].data.get("hit"), True)
            for action_id in ("s1", "s2"):
                self.assertEqual(_status_code(oks[action_id].data, DIRT), "M")
            cleanups = [e for e in events if e.type == CLEANUP_REMOVED]
            self.assertEqual(len(cleanups), 1)
            self.assertEqual(
                (cleanups[0].data.get("miss"), cleanups[0].data.get("leak")),
                (False, False),
                cleanups[0].data,
            )

    def test_blocked_wfpw_status_honest(self) -> None:
        """Parked sync gate heals across checkout; either verdict informs.

        A reject here is the first hunt task (only a missing expected
        modification may reject); an accept documents the race does not
        fire on current master. Both pin the same deterministic
        composition and witness, so any other outcome fails.
        """
        spec = _wfpw_spec(REGISTERS[0])
        with _artifact_directory("fault-wfpw-") as tmp:
            result = run_live(spec, tmp)
            _fail_on_setup_failure(result, tmp)
            self.assertEqual(
                result.execution.status,
                COMPLETED,
                result.execution,
            )
            self.assertEqual(result.execution.reason, "ok")
            events = load_history(os.path.join(tmp, "history.jsonl"))
            invokes = {e.data["action"]: e for e in events if e.type == INVOKE}
            oks = {e.data["action"]: e for e in events if e.type == OK}
            # The witness fires while parked; the checkout heals across
            # the park (its completion precedes the read's). No order is
            # asserted between the remover and the read: a fast target
            # can complete on the unblock before remove returns.
            self.assertLess(invokes["t1"].time, oks["h1"].time)
            self.assertLess(oks["c0"].time, oks["t1"].time)
            self.assertEqual(oks["h1"].data.get("hit"), True)
            if result.verdict.kind == REJECT:
                self.assertLessEqual(
                    _violated_rules(result.verdict),
                    _STALE_STATUS_RULES,
                    _unsatisfied(result.verdict),
                )
            else:
                _assert_accept(self, result.verdict)
                self.assertEqual(_status_code(oks["s2"].data, DIRT), "M")
            cleanups = [e for e in events if e.type == CLEANUP_REMOVED]
            self.assertEqual(len(cleanups), 1)
            self.assertEqual(
                (cleanups[0].data.get("miss"), cleanups[0].data.get("leak")),
                (False, False),
                cleanups[0].data,
            )

    def test_await_without_install_witness_failed(self) -> None:
        """Awaiting a never-installed fault fails without touching the daemon."""
        env = Environment(seed=_SEED, registers=REGISTERS, sink=InMemorySink())
        self.addCleanup(env.close)
        env.setup()
        register = REGISTERS[0]
        spec = _spec(register, [], [])
        backend = LiveBackend(env, spec)
        with self.assertRaises(HarnessFailure) as ctx:
            backend(_await("h1", "F"))
        self.assertEqual(ctx.exception.reason, FAULT_WITNESS_FAILED)

    def test_runner_kill_while_parked(self) -> None:
        """Killing the owned checkout records raw failure and releases the fault.

        The fault-remove backend call marks that the checkout is parked and
        waits for the supervisor. The supervisor kills only the sl process descended
        from this run's exact spec runner, then releases removal. The checkout
        exit, fault witness, cleanup, and manifests remain observable.
        """
        spec = _green_spec(REGISTERS[0])
        results: list[ExplorationResult] = []
        with _artifact_directory("fault-kill-") as tmp:
            spec_path = os.path.join(tmp, "run-spec.json")
            ready = os.path.join(tmp, "fault-remove-ready")
            release = os.path.join(tmp, "fault-remove-release")
            runner_argv = runner_argv_for(spec_path, EDENFS_TARGET.name)
            runner_argv[1] = os.path.abspath(__file__)
            runner_argv.extend(
                [
                    "--release-at-remove",
                    "--ready-marker",
                    ready,
                    "--release-marker",
                    release,
                ]
            )
            worker = threading.Thread(
                target=lambda: results.append(
                    run_exploration(
                        spec,
                        tmp,
                        target=EDENFS_TARGET,
                        runner_argv=runner_argv,
                    )
                ),
                daemon=True,
            )
            worker.start()
            node = _wait_live_ready(tmp, worker)
            try:
                _wait_marker(ready, worker)
                killed_pids = _wait_sl_child(node, spec_path, worker)
                for pid in killed_pids:
                    os.kill(pid, signal.SIGKILL)
            finally:
                with open(release, "w") as handle:
                    handle.write("release\n")
            worker.join(timeout=180.0)
            self.assertFalse(worker.is_alive(), "run stuck after the checkout died")
            (result,) = results
            self.assertEqual(
                result.execution.status,
                COMPLETED,
                result.execution,
            )
            self.assertEqual(result.execution.reason, "ok", result.execution)
            _assert_accept(self, result.verdict)
            events = load_history(os.path.join(tmp, "history.jsonl"))
            failures = {
                e.data["action"]: e
                for e in events
                if e.type == FAIL and "action" in e.data
            }
            oks = {e.data["action"]: e for e in events if e.type == OK}
            self.assertEqual(failures["c0"].data.get("returncode"), -signal.SIGKILL)
            self.assertIn("stdout", failures["c0"].data)
            self.assertIn("stderr", failures["c0"].data)
            self.assertEqual(oks["h1"].data.get("hit"), True, result.execution)
            cleanups = [e for e in events if e.type == CLEANUP]
            self.assertEqual(len(cleanups), 1, result.execution)
            self.assertEqual(
                cleanups[0].data.get("unwind", {}).get("leaks"),
                0,
                result.execution,
            )
            self.assertIsNone(cleanups[0].data.get("close_error"), result.execution)
            names = (
                "run-spec.json",
                "history.jsonl",
                "execution.json",
                "verdict.json",
                "replay.json",
            )
            for name in names:
                self.assertTrue(os.path.isfile(os.path.join(tmp, name)), name)
            print(f"exploratory artifacts: {tmp}")


BACKGROUND_READ_CAMPAIGN = "fault-live-background-read"


class BackgroundReadLiveTest(unittest.TestCase):
    def setUp(self) -> None:
        _require_live(self)

    def test_background_read_overlaps_park_and_drains(self) -> None:
        """An owned directory reader overlaps a witnessed park and is fully reaped."""
        actions = [
            Action(
                "b0",
                "read",
                DEADLINE_OPERATION,
                payload={"path": REGISTERS[0], "channel": "fuse"},
            ),
            replace(_install("c0"), requires=(("b0", FINISHED),)),
            _checkout("c0", 2, ("f1", FINISHED)),
            _await("h1", "F", ("c0", STARTED)),
            Action(
                "l1",
                "background_read",
                DEADLINE_OPERATION,
                (("h1", FINISHED),),
                {"root": "tree", "seconds": 20},
            ),
            Action(
                "k1",
                "kill_process",
                DEADLINE_OPERATION,
                (("l1", STARTED),),
                {"target_action": "l1", "after_ms": 500},
            ),
            Action(
                "x1",
                "fault_remove",
                DEADLINE_OPERATION,
                (("k1", FINISHED), ("l1", FINISHED)),
                {"fault_id": "F"},
            ),
        ]
        spec = _spec(
            REGISTERS[0],
            actions,
            [
                ("fault", ["b0", "f1", "h1", "k1", "x1"]),
                ("checkout", ["c0"]),
                ("load", ["l1"]),
            ],
        )
        validate(spec)
        with _artifact_directory("background-read-") as artifacts:
            provenance, degradation = _campaign_provenance(EDENFS_TARGET.name)
            result = _run_campaign_seed(
                spec,
                artifacts,
                EDENFS_TARGET,
                provenance,
                degradation,
                KEEP_ALL,
                BACKGROUND_READ_CAMPAIGN,
            )
            self.assertEqual(COMPLETED, result.execution.status, result.execution)
            _assert_accept(self, result.verdict)
            self.assertEqual([], _unsatisfied(result.verdict))
            admission = load_admission(Path(artifacts))
            self.assertEqual(
                (RunPurpose.EXPLORE, BACKGROUND_READ_CAMPAIGN),
                (admission.run_purpose, admission.campaign_id),
            )
            events = load_history(os.path.join(artifacts, "history.jsonl"))
            invokes = {e.data["action"]: e for e in events if e.type == INVOKE}
            ends = {e.data["action"]: e for e in events if e.type in {OK, FAIL}}
            self.assertTrue(ends["h1"].data["hit"])
            self.assertEqual(
                ends["f1"].data["selector"], re.escape(ends["h1"].data["witness"])
            )
            self.assertLess(ends["h1"].time, invokes["l1"].time)
            self.assertLess(ends["l1"].time, ends["c0"].time)
            self.assertLess(invokes["x1"].time, ends["c0"].time)
            self.assertLess(invokes["l1"].time, invokes["x1"].time)
            self.assertLess(ends["l1"].time, invokes["x1"].time)
            self.assertEqual(FAIL, ends["l1"].type)
            self.assertEqual(-signal.SIGKILL, ends["l1"].data["returncode"])
            pgid = ends["l1"].data["pgid"]
            self.assertEqual(pgid, ends["k1"].data["pgid"])
            self.assertTrue(ends["k1"].data["group_empty"])
            with self.assertRaises(ProcessLookupError):
                os.killpg(pgid, 0)
            self.assertEqual(0, ends["x1"].data["removed"])
            cleanup = next(e for e in events if e.type == CLEANUP)
            self.assertEqual(0, cleanup.data["unwind"]["leaks"])
            self.assertIsNone(cleanup.data["close_error"])
            self.assertEqual(OK, ends["c0"].type)
            print(
                f"background-read pgid={pgid} sweeps={ends['l1'].data['sweeps']} artifacts={artifacts}"
            )


if __name__ == "__main__" and "--checkpoint-fixture" in sys.argv:
    raise SystemExit(_checkpoint_runner_main(_argument_value(sys.argv, "--spec")))


if __name__ == "__main__" and "--release-at-remove" in sys.argv:
    raise SystemExit(
        _release_runner_main(
            _argument_value(sys.argv, "--spec"),
            _argument_value(sys.argv, "--ready-marker"),
            _argument_value(sys.argv, "--release-marker"),
        )
    )
