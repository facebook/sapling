#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import contextlib
import os
import subprocess
import sys
import time
import uuid
from typing import Iterator

from eden.fs.cli.telemetry import TelemetrySample

CLAUDE_TIMEOUT_SECS = 120


@contextlib.contextmanager
def logged_sample(sample: TelemetrySample) -> Iterator[None]:
    """Log `sample` exactly once, however the command ends.

    `KeyboardInterrupt` gets its own arm because it is the likely outcome of a
    user giving up on the claude wait, and it does not derive from `Exception`.

    Deliberately not `with instance.get_telemetry_logger().new_sample(...)`:
    that form's `__exit__` overwrites `duration` with the whole command's wall
    time, and `duration` here belongs to the claude subprocess alone.
    """
    sample.add_string("reason", "unhandled_exception")
    try:
        yield
    except KeyboardInterrupt:
        sample.add_string("reason", "interrupted")
        sample.fail("interrupted")
        raise
    except BaseException as ex:
        sample.fail(str(ex))
        raise
    finally:
        sample.log()


def run_claude(prompt: str, timeout_secs: int, sample: TelemetrySample) -> bool:
    """Ask a local `claude` for a diagnosis, writing whatever it produces through.

    Records `reason`, `success`, `duration` and `claude_session_id` on `sample`,
    and returns whether a diagnosis came back.
    """
    claude_env = os.environ.copy()
    claude_env.pop("CLAUDECODE", None)
    # Choosing the session id rather than parsing it back out of `claude`
    # keeps its output untouched, and lets a row be traced to the
    # conversation that produced the diagnosis.
    claude_session_id = str(uuid.uuid4())
    sample.add_string("claude_session_id", claude_session_id)
    claude_start = time.monotonic()
    try:
        claude_result = subprocess.run(
            ["claude", "--print", "--session-id", claude_session_id],
            input=prompt,
            capture_output=True,
            text=True,
            timeout=timeout_secs,
            env=claude_env,
            check=False,
        )
    except FileNotFoundError:
        sample.add_string("reason", "claude_not_on_path")
        sample.add_bool("success", False)
        print(
            "Local `claude` was not found on PATH; skipping AI diagnosis.",
            file=sys.stderr,
        )
        return False
    except OSError as ex:
        # `claude` exists but could not be started, e.g. it is not
        # executable. Must follow FileNotFoundError, which subclasses this.
        sample.add_string("reason", "claude_failed")
        sample.add_bool("success", False)
        sample.add_double("duration", time.monotonic() - claude_start)
        print(
            f"Local `claude` could not be started: {ex}",
            file=sys.stderr,
        )
        return False
    except subprocess.TimeoutExpired as ex:
        sample.add_string("reason", "claude_timed_out")
        sample.add_bool("success", False)
        sample.add_double("duration", time.monotonic() - claude_start)
        print(
            "Local `claude` timed out while generating the diagnosis.",
            file=sys.stderr,
        )
        _write_through(_as_text(ex.stdout), _as_text(ex.stderr))
        return False

    sample.add_double("duration", time.monotonic() - claude_start)
    succeeded = claude_result.returncode == 0
    sample.add_bool("success", succeeded)
    sample.add_string("reason", "claude_ok" if succeeded else "claude_failed")
    if not succeeded:
        print(
            "Local `claude` failed while generating the diagnosis.",
            file=sys.stderr,
        )
    _write_through(claude_result.stdout, claude_result.stderr)
    return succeeded


def _as_text(stream: str | bytes | None) -> str:
    # TimeoutExpired carries bytes even when the subprocess ran in text mode.
    if isinstance(stream, bytes):
        return stream.decode(errors="replace")
    return stream or ""


def _write_through(stdout: str, stderr: str) -> None:
    if stdout:
        sys.stdout.write(stdout)
    if stderr:
        sys.stderr.write(stderr)
