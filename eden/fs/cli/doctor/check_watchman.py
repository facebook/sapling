#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import collections
import io
import json
import logging
import subprocess
import typing
from typing import Any, Dict, List, Optional, Set

from eden.fs.cli.doctor.problem import (
    FixableProblem,
    Problem,
    ProblemTracker,
    RemediationError,
)


log: logging.Logger = logging.getLogger("eden.fs.cli.doctor.checks.watchman")


WatchmanCheckInfo = collections.namedtuple("WatchmanCheckInfo", ["watchman_roots"])


def pre_check() -> WatchmanCheckInfo:
    watchman_roots = _get_watch_roots_for_watchman()
    return WatchmanCheckInfo(watchman_roots)


def check_active_mount(
    tracker: ProblemTracker, path: str, info: WatchmanCheckInfo
) -> None:
    check_watchman_subscriptions(tracker, path, info)


class IncorrectWatchmanWatch(FixableProblem):
    def __init__(self, path: str, watcher: Any) -> None:
        self._path = path
        self._watcher = watcher

    def description(self) -> str:
        return (
            f"Watchman is watching {self._path} with the wrong watcher type: "
            f'"{self._watcher}" instead of "eden"'
        )

    def dry_run_msg(self) -> str:
        return f"Would fix watchman watch for {self._path}"

    def start_msg(self) -> str:
        return f"Fixing watchman watch for {self._path}"

    def perform_fix(self) -> None:
        # Delete the old watch and try to re-establish it. Hopefully it will be
        # an EdenFS watch this time.
        _call_watchman(["watch-del", self._path])
        watch_details = _call_watchman(["watch-project", self._path])
        if watch_details.get("watcher") != "eden":
            raise RemediationError(
                f"Failed to replace watchman watch for {self._path} "
                'with an "eden" watcher'
            )


class MissingOrDuplicatedSubscription(Problem):
    def __init__(self, message: str) -> None:
        super().__init__(message)


def check_watchman_subscriptions(
    tracker: ProblemTracker, path: str, info: WatchmanCheckInfo
) -> None:
    if path not in info.watchman_roots:
        return

    watch_details = _call_watchman(["watch-project", path])
    watcher = watch_details.get("watcher")
    if watcher == "eden":
        return

    tracker.add_problem(IncorrectWatchmanWatch(path, watcher))


def _get_watch_roots_for_watchman() -> Set[str]:
    js = _call_watchman(["watch-list"])
    roots = set(js.get("roots", []))
    return roots


def _call_watchman(args: List[str]) -> Dict:
    full_args = ["watchman"]
    full_args.extend(args)
    return _check_json_output(full_args)


def _check_json_output(args: List[str]) -> Dict[str, Any]:
    """Calls subprocess.check_output() and returns the output parsed as JSON.
    If the call fails, it will write the error to stderr and return a dict with
    a single property named "error".
    """
    try:
        output = subprocess.check_output(args)
        return typing.cast(Dict[str, Any], json.loads(output))
    except FileNotFoundError as e:
        # Same as below, but we don't need to emit a warning if they don't have
        # nuclide-connections installed.
        errstr = getattr(e, "strerror", str(e))
        return {"error": str(e)}
    except Exception as e:
        # FileNotFoundError if the command is not found.
        # CalledProcessError if the command exits unsuccessfully.
        # ValueError if `output` is not valid JSON.
        errstr = getattr(e, "strerror", str(e))
        log.warning(f'Calling `{" ".join(args)}` failed with: {errstr}')
        return {"error": str(e)}
