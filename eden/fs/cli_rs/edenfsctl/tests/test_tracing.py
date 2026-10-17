# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import base64
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest

from artillery.artillery_trace_model.thrift_types import Event_TulipWrapper, EventType
from facebook.contextprop.Baggage.thrift_types import Baggage
from facebook.contextprop.py3.ContextpropCore.thrift_types import (
    ArtilleryPropagatedContext,
)
from thrift.python.serializer import deserialize, Protocol

TRACE = "5ad4f2c7a3e14b8d1f2e3d4c5b6a7940"
AGENT = "AGENT:claude_code"
CERT = """\
-----BEGIN CERTIFICATE-----
MIIDSzCCAjOgAwIBAgIUBZH3MYYDAtH3rw4GEoicCwbFgyowDQYJKoZIhvcNAQEL
BQAwGDEWMBQGA1UEAwwNY2xpX2xpdGVfdGVzdDAgFw0yNjA5MDEwNTI2MDNaGA8y
MTI2MDgwODA1MjYwM1owGDEWMBQGA1UEAwwNY2xpX2xpdGVfdGVzdDCCASIwDQYJ
KoZIhvcNAQEBBQADggEPADCCAQoCggEBAKXL9A3z58/dnw9duMi1pjARHnDzMRRJ
1dFIGDMqJVN8O0ngpQdl2DM4dN1i32JArfIquwULAC1izthntgFo0uXNSNpRaclj
2oggdKXIwsWYKyK99q2dBi77kq5yVTLArBRVWrz1Q5/EMfyGDsxP/lsJ30Zy5tRo
KXACiJ/Vgo8pScQ/8cBKvNtUkcNQCUwRqNMfj0gLNZHtu1H5O6h0ps3OTZVGrpyl
zGO2QQAjt0dV1uxRcwtMhwfl11xjRo8vjFXCNmPtdsN00uUoyR0nfto1ZJ0zNCxB
SnC0fnO0+wIIjL/U77IYEtVRb1s8pWfYuL87D77VLKAG00hN9kAQ74sCAwEAAaOB
ijCBhzBmBgNVHREEXzBdhkFtaWQ6Ly9QUk9EL1VTRVIvMTIzND9mbGF2b3I9eCZh
Z2VudC5pZD1BR0VOVCUzYWNsYXVkZV9jb2RlJnRpZXI9eYYYbWlkOi8vUFJPRC9N
QUNISU5FL2hvc3QwMB0GA1UdDgQWBBTWPEQZLYG+v7eXfhis6aMrM+bJHTANBgkq
hkiG9w0BAQsFAAOCAQEApL49k77CDcILMCZd+aY2W70PG4OkbexS0ovBg5Zfok43
GMWkG612VKBcJOpymvs2sscvdVZNpb/szTNMBtlKsVaS9vpr5fGzm+34OH+dUZ7o
Apz+0yPCYeHLvErrmXf9zWZJ8usFzipWgqFcFNpeyQ/7p02p89/fVnKrzWIYarZl
/+6w9wq4XM0+SkapyRdW0Q+LqpvVIBQJUIOgY+pD1OMPJCIpuEwh5QJYc8C2Dae2
8c+HXC0sz0TCyWiriU3ErKL9BQObu/wrDYbRt8YWOryHp0Zvx7M8p9JxuOFoEgJE
ox+ZgOXJBgPJwkSmw7K5ugovFBuStJFUB5jaM2FYrQ==
-----END CERTIFICATE-----
"""
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
)
CARRIERS = ("CONTEXTPROP_BAGGAGE", "ARTILLERY_CONTEXT", "TRACEPARENT")
CMD_DUMP = """@echo off
echo helper stdout
echo helper stderr 1>&2
set > "%~1"
"""
SH_DUMP = """#!/bin/sh
echo helper stdout
echo helper stderr >&2
exec env -0 > "$1"
"""
PY_SCRIBE = """import os, sys, tempfile
slot = tempfile.mkdtemp(dir=os.path.dirname(os.path.abspath(__file__)))
with open(os.path.join(slot, "batch"), "wb") as f:
    f.write(sys.stdin.buffer.read())
os.replace(os.path.join(slot, "batch"), os.path.join(slot, "scribe.txt"))
"""
SH_SCRIBE = """#!/bin/sh
d=$(mktemp -d "${0%/*}/slot.XXXXXX")
cat > "$d/batch"
mv "$d/batch" "$d/scribe.txt"
"""


def write_script(directory: str, name: str, cmd: str, sh: str) -> str:
    path = os.path.join(directory, name + (".cmd" if os.name == "nt" else ".sh"))
    with open(path, "w") as f:
        f.write(cmd if os.name == "nt" else sh)
    os.chmod(path, 0o755)
    return path


def shipped(directory: str) -> list[bytes]:
    events = []
    for root, _, names in os.walk(directory):
        if "scribe.txt" in names:
            with open(os.path.join(root, "scribe.txt"), "rb") as f:
                events += [base64.b64decode(line) for line in f.read().split()]
    return events


def root_block(directory: str) -> dict[str, str]:
    for _ in range(100):
        events = [
            deserialize(Event_TulipWrapper, e[2:], protocol=Protocol.COMPACT).payload
            for e in shipped(directory)
        ]
        notes = [e for e in events if e.type == EventType.ANNOTATE_BLOCK]
        roots = {
            e.id1
            for e in notes
            if e.annotations.get("b:name") == "cli.edenfsctl"
            and e.annotations.get("b:is_service_block") == "True"
        }
        ended = {e.id2 for e in events if e.type == EventType.CREATE_BLOCK_END}
        for block in roots & ended:
            return {
                k: v for e in notes if e.id1 == block for k, v in e.annotations.items()
            }
        time.sleep(0.1)
    return {}


def read_env(path: str) -> dict[str, str]:
    with open(path, "rb") as f:
        data = f.read()
    separator = b"\r\n" if os.name == "nt" else b"\0"
    return dict(
        entry.decode().split("=", 1) for entry in data.split(separator) if entry
    )


def decoded(value: str) -> bytes:
    value = value.replace("+", "-").replace("/", "_")
    return base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))


def agent_ids(child: dict[str, str]) -> tuple[str | None, str | None]:
    apc = deserialize(
        ArtilleryPropagatedContext,
        decoded(child["ARTILLERY_CONTEXT"]),
        protocol=Protocol.COMPACT,
    )
    cp2 = deserialize(
        Baggage, decoded(child["CONTEXTPROP_BAGGAGE"]), protocol=Protocol.COMPACT
    )
    context = cp2.trace_context
    assert context is not None
    tracing = context.tracing_context
    assert tracing is not None
    assert apc.trace_id == int.from_bytes(bytes.fromhex(TRACE[16:]), "little")
    assert (tracing.trace_id, tracing.edge_id) == (apc.trace_id, apc.edge_id)
    return apc.agent_id, cp2.agent_id


def without_carriers(env: dict[str, str]) -> dict[str, str]:
    return {k: v for k, v in env.items() if k not in CARRIERS}


class EdenfsctlTracingTest(unittest.TestCase):
    def setUp(self) -> None:
        self.dir = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.addCleanup(self.dir.cleanup)
        self.binary = os.path.abspath(os.environ["EDENFSCTL"])
        self.eden = os.path.join(self.dir.name, "eden")
        try:
            os.symlink(self.binary, self.eden)
        except OSError:
            self.eden = self.binary
        self.helper = [write_script(self.dir.name, "dump", CMD_DUMP, SH_DUMP)]
        self.out = os.path.join(self.dir.name, "env.json")

    def environment(self, control: dict[str, object] | None) -> dict[str, str]:
        env = {k: v for k, v in os.environ.items() if k not in AMBIENT}
        env.update(
            EDENFS_NO_TELEMETRY="1",
            EDENFSCTL_REAL=" ".join([*self.helper, self.out]),
            KMP_LIBRARY="multiuser",
            KMP_AFFINITY="disabled",
            LIBOMP_NUM_HIDDEN_HELPER_THREADS="0",
            LIBOMP_USE_HIDDEN_HELPER_TASK="0",
        )
        env.pop("PYTHONHOME", None)
        env.pop("PYTHONPATH", None)
        if sys.platform == "darwin":
            env["__CF_USER_TEXT_ENCODING"] = f"0x{os.getuid():X}:0:0"
        if control is not None:
            path = os.path.join(self.dir.name, "control.json")
            with open(path, "w") as f:
                json.dump(
                    {"transports": ["none"], "observe_sample_rate": 0, **control}, f
                )
            cert = os.path.join(self.dir.name, "cert.pem")
            with open(cert, "w") as f:
                f.write(CERT)
            env.update(
                TRACEPARENT=f"00-{TRACE}-b7ad6b7169203331-01",
                CODING_AGENT_METADATA="id=claude_code",
                ARTILLERY_KILLSWITCH_PATH=path,
                THRIFT_TLS_CL_CERT_PATH=cert,
            )
        return env

    def exit_run(
        self,
        args: list[str],
        control: dict[str, object] | None,
        extra: dict[str, str],
    ) -> tuple[str, tuple[int, str, str]]:
        directory = tempfile.mkdtemp(dir=self.dir.name)
        env = self.environment(control)
        env.update(
            extra,
            EDENFSCTL_REAL=write_script(
                directory, "exit3", "@exit /b 3\n", "#!/bin/sh\nexit 3\n"
            ),
        )
        if os.name == "nt":
            with open(os.path.join(directory, "artillery_trace_events"), "w") as f:
                f.write(PY_SCRIBE)
            env["SCRIBE_CAT_PATH"] = sys.executable
        else:
            env["SCRIBE_CAT_PATH"] = write_script(
                directory, "scribe_cat", "", SH_SCRIBE
            )
        done = subprocess.run(
            [self.binary, *args],
            env=env,
            cwd=directory,
            capture_output=True,
            text=True,
        )
        return directory, (done.returncode, done.stdout, done.stderr)

    def run_helper(
        self, argv: list[str], env: dict[str, str]
    ) -> tuple[int, str, str, dict[str, str]]:
        done = subprocess.run(argv, env=env, capture_output=True, text=True)
        child = read_env(self.out)
        os.unlink(self.out)
        return done.returncode, done.stdout, done.stderr, child

    def baseline_and_fallback(
        self, name: str, control: dict[str, object] | None
    ) -> tuple[
        tuple[int, str, str, dict[str, str]], tuple[int, str, str, dict[str, str]]
    ]:
        env = self.environment(control)
        bare = self.run_helper([*self.helper, self.out], env)
        return bare, self.run_helper([name, "info"], env)

    def test_an_agent_reaches_python_edenfsctl_continuing_its_trace(self) -> None:
        for name in (self.eden, self.binary):
            with self.subTest(name=os.path.basename(name)):
                bare, traced = self.baseline_and_fallback(
                    name,
                    {"killed": True, "overrides": {"edenfsctl": {"killed": False}}},
                )
                self.assertEqual(traced[:3], bare[:3])
                child = traced[3]
                self.assertEqual(agent_ids(child), (AGENT, AGENT))
                self.assertEqual(child["TRACEPARENT"], "")
                self.assertEqual(without_carriers(child), without_carriers(bare[3]))

    def assert_unchanged(self, control: dict[str, object] | None) -> None:
        for name in (self.eden, self.binary):
            with self.subTest(name=os.path.basename(name)):
                bare, run = self.baseline_and_fallback(name, control)
                self.assertEqual(run, bare)

    def test_a_human_reaches_python_edenfsctl_unchanged(self) -> None:
        self.assert_unchanged(None)

    def test_a_killed_edenfsctl_hands_python_an_unchanged_environment(self) -> None:
        self.assert_unchanged(
            {"killed": False, "overrides": {"edenfsctl": {"killed": True}}}
        )

    def test_a_sampled_out_edenfsctl_hands_python_an_unchanged_environment(
        self,
    ) -> None:
        self.assert_unchanged(
            {"killed": False, "overrides": {"edenfsctl": {"sample_rate": 0.0}}}
        )

    def test_every_exit_ships_the_root_block_and_keeps_its_output(self) -> None:
        traced: dict[str, object] = {
            "transports": ["scribe_cat"],
            "killed": True,
            "overrides": {"edenfsctl": {"killed": False}},
        }
        killed: dict[str, object] = {
            "transports": ["scribe_cat"],
            "killed": False,
            "overrides": {"edenfsctl": {"killed": True}},
        }
        rust = {"EDENFSCTL_ONLY_RUST": "1"}
        cases: tuple[tuple[list[str], dict[str, str], int], ...] = (
            (["info"], {}, 3),
            (["list", "--help"], {}, 0),
            (["--help"], rust, 0),
            (["--no-such-flag"], rust, 64),
            (["redirect", "add"], rust, 2),
        )
        for args, extra, code in cases:
            with self.subTest(args=args, only_rust=bool(extra)):
                untraced, human = self.exit_run(args, None, extra)
                self.assertEqual(human[0], code)
                directory, run = self.exit_run(args, killed, extra)
                self.assertEqual(run, human)
                time.sleep(1)
                self.assertEqual(shipped(untraced), [])
                self.assertEqual(shipped(directory), [])
                directory, run = self.exit_run(args, traced, extra)
                self.assertEqual(run, human)
                root = root_block(directory)
                self.assertEqual(
                    root.get("b:name"), "cli.edenfsctl", "no ended root block"
                )
                self.assertEqual(root.get("u:ai_agent_id"), AGENT, root)
