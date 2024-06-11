# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

import os
import subprocess
import sys
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
            self.socket = self._watchman_dir / "sock"
            self._close_fds = True

        self.config = self._watchman_dir / "config.json"

    def start(self) -> None:
        with open(self.config, "w", encoding="utf8") as f:
            f.write('{"min_acceptable_nice_value": 999}')

        clilogfile = str(self._watchman_dir / "cli-log")
        logfile = str(self._watchman_dir / "log")
        pidfile = str(self._watchman_dir / "pid")
        statefile = str(self._watchman_dir / "state")

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
        self.generate_watchman_cli_wrapper(
            self._watchman_dir,
            [self._watchman_bin, "--no-spawn", "--no-local", "--sockname", self.socket],
            env,
        )
        deadline = time.time() + 30
        watchmanavailable = False
        lastoutput = ""
        while not watchmanavailable and time.time() < deadline:
            try:
                # The watchman CLI can wait for a short time if socket
                # is not ready.
                subprocess.run(
                    argv,
                    env=env,
                    close_fds=self._close_fds,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    check=True,
                )
                watchmanavailable = True
            except subprocess.CalledProcessError as ex:
                time.sleep(0.1)
                lastoutput = ex.output.decode(errors="replace")
        if not watchmanavailable:
            sys.stderr.write(f"watchman CLI output: {lastoutput}\n")
            sys.stderr.write("watchman log:\n%s\n" % open(clilogfile, "r").read())
            raise WatchmanTimeout()

    def stop(self) -> None:
        process = self._watchman_proc
        if process is not None:
            process.terminate()
            process.kill()

    def generate_watchman_cli_wrapper(self, watchmanpath: Path, cmd, env):
        cmd = [str(s) for s in cmd]
        binpath = watchmanpath.parents[1] / "install" / "bin" / "watchmanscript"
        env = env.copy()
        # These two are annoying to escape, so let's just get rid of them
        env.pop("HGTEST_EXCLUDED", None)
        env.pop("HGTEST_INCLUDED", None)
        if not os.name == "nt":
            with open(binpath, "w") as f:
                f.write("#!/usr/bin/env bash\n")
                for k, v in env.items():
                    f.write(f"export {k}={repr(v)}\n")
                f.write(" ".join(cmd) + ' "$@"\n')
            os.chmod(binpath, 0o775)
        else:
            with open(str(binpath) + ".bat", "w") as f:
                f.write("@echo off\n")
                for k, v in env.items():
                    f.write(f"set {k}={v}\n")
                cmd[0] = f'"{cmd[0]}"'
                fullpath = (" ".join(cmd)).strip()
                f.write(f"{fullpath} %*\n")
                f.write("exit /B %errorlevel%\n")
