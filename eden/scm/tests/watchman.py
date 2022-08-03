# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

import os
import subprocess
import time
import uuid
from pathlib import Path
from typing import Optional


class WatchmanTimeout(Exception):
    pass


class Watchman:
    config: Path
    socket: Path

    _watchman_bin: Path
    _watchman_dir: Path
    _watchman_proc: Optional[subprocess.Popen[bytes]]
    _close_fds: bool

    def __init__(self, watchman_bin: Path, watchman_dir: Path) -> None:
        self._watchman_bin = watchman_bin
        self._watchman_dir = watchman_dir
        self._watchman_proc = None

        if os.name == "nt":
            self.socket = Path("\\\\.\\pipe\\watchman-test-%s" % uuid.uuid4().hex)
            self._close_fds = False
        else:
            self.socket = Path(os.path.join(self._watchman_dir, "sock"))
            self._close_fds = True

        self.config = Path(os.path.join(self._watchman_dir, "config.json"))

    def start(self) -> None:
        with open(self.config, "w", encoding="utf8") as f:
            f.write("{}")

        clilogfile = os.path.join(self._watchman_dir, "cli-log")
        logfile = os.path.join(self._watchman_dir, "log")
        pidfile = os.path.join(self._watchman_dir, "pid")
        statefile = os.path.join(self._watchman_dir, "state")

        env = os.environ.copy()
        env["WATCHMAN_CONFIG_FILE"] = str(self.config)
        env["WATCHMAN_SOCK"] = str(self.socket)

        argv = [
            self._watchman_bin,
            "--sockname",
            self.socket,
            "--logfile",
            logfile,
            "--pidfile",
            pidfile,
            "--statefile",
            statefile,
            "--foreground",
            "--log-level=2",  # debug logging for watchman
        ]

        with open(clilogfile, "wb") as f:
            self._watchman_proc = subprocess.Popen(
                argv, env=env, stdin=None, stdout=f, stderr=f, close_fds=self._close_fds
            )

        # Wait for watchman socket to become available
        argv = [
            self._watchman_bin,
            "--no-spawn",
            "--no-local",
            "--sockname",
            self.socket,
            "version",
        ]
        deadline = time.time() + 30
        watchmanavailable = False
        while not watchmanavailable and time.time() < deadline:
            try:
                # The watchman CLI can wait for a short time if socket
                # is not ready.
                subprocess.check_output(argv, env=env, close_fds=self._close_fds)
                watchmanavailable = True
            except Exception:
                time.sleep(0.1)
        if not watchmanavailable:
            raise WatchmanTimeout()

    def stop(self) -> None:
        process = self._watchman_proc
        if process is not None:
            process.terminate()
            process.kill()
