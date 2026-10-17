# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os
import subprocess
import sys
import tempfile
import time
import unittest

AMBIENT = (
    "TRACEPARENT",
    "CONTEXTPROP_BAGGAGE",
    "ARTILLERY_CONTEXT",
    "CLAUDE_CODE_CURRENT_SESSION_ID",
    "CC_TRACE_DIR",
    "CODING_AGENT_METADATA",
    "META_3PAI_INVOCATION_ID",
    "AGENT",
    "CONFUCIUS_IN_NAMESPACE",
    "ARTILLERY_KILLSWITCH_PATH",
    "THRIFT_TLS_CL_CERT_PATH",
    "THRIFT_TLS_CL_KEY_PATH",
)
RUNNER = ("FB_PAR_", "FB_XAR_", "PYTHON", "IMPORTS_MONITOR_")


class InterruptExitTest(unittest.TestCase):
    def interrupt(self, fault: str, traced: bool = True) -> tuple[int, str, str, float]:
        directory = tempfile.mkdtemp()
        started = os.path.join(directory, "started")
        env = {
            k: v
            for k, v in os.environ.items()
            if k not in AMBIENT and not k.startswith(RUNNER)
        }
        env.update(EDENFS_NO_TELEMETRY="1", INTERRUPT_PROBE_STARTED=started)
        if traced:
            control = os.path.join(directory, "control.json")
            with open(control, "w") as f:
                json.dump(
                    {
                        "transports": ["none"],
                        "observe_sample_rate": 0,
                        "killed": False,
                        "overrides": {"edenfsctl": {"sample_rate": 1.0}},
                    },
                    f,
                )
            env.update(
                TRACEPARENT="00-5ad4f2c7a3e14b8d1f2e3d4c5b6a7940-b7ad6b7169203331-01",
                CODING_AGENT_METADATA="id=claude_code",
                ARTILLERY_KILLSWITCH_PATH=control,
            )
        probe = os.environ["INTERRUPT_PROBE"]
        launcher = [sys.executable] if os.name == "nt" else []
        done = subprocess.run(
            [*launcher, probe, fault],
            env=env,
            capture_output=True,
            text=True,
            timeout=120,
        )
        ended = time.monotonic()
        self.assertEqual(
            (done.returncode, done.stdout, done.stderr),
            (130, "", ""),
            f"probe {fault} traced={traced}",
        )
        with open(started) as f:
            return done.returncode, done.stdout, done.stderr, ended - float(f.read())

    def assert_exits_130(
        self, fault: str, traced: bool, low: float, high: float
    ) -> None:
        code, stdout, stderr, elapsed = self.interrupt(fault, traced)
        self.assertEqual((code, stdout, stderr), (130, "", ""))
        self.assertGreaterEqual(elapsed, low)
        self.assertLess(elapsed, high)

    def test_a_traced_ctrl_c_exits_130_within_the_bound(self) -> None:
        self.assert_exits_130("finish", traced=True, low=0.0, high=1.5)

    def test_an_untraced_ctrl_c_exits_130_at_once(self) -> None:
        self.assert_exits_130("finish", traced=False, low=0.0, high=0.5)

    def test_a_finisher_that_cannot_start_exits_130(self) -> None:
        self.assert_exits_130("start_failure", traced=True, low=0.0, high=0.5)

    def test_a_second_interrupt_while_finishing_exits_130(self) -> None:
        self.assert_exits_130("second_interrupt", traced=True, low=0.0, high=0.5)

    def test_a_hung_block_shipment_exits_130_after_the_one_second_bound(self) -> None:
        self.assert_exits_130("hang", traced=True, low=1.0, high=1.6)
