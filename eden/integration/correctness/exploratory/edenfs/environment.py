# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Minimal isolated EdenFS daemon plus backing-repo fixture.

Owns one throwaway daemon, one generated backing repo whose every revision
carries that revision's 64-byte image of each spec register (and rev0 the
revision-stable churn files), and one mounted clone, all under a
``run.*`` directory created inside the given ``run_dir`` (or, when none is
given, inside a fresh ``eden_exploratory.*`` temp dir). Exposes
``mount_path``, ``commits``, ``backing_repo``, and ``get_thrift_client``.
With ``reference_clone`` it also owns the differential reference checkout,
prepared after every other setup step so it starts in the mount's modeled
state. Setup ends by emitting a ``target-setup`` event carrying the setup
facts, so they are durable before any action runs. A contained run never
closes the environment: the supervisor kills and removes everything under
``run_dir``; ``close`` serves in-process callers.
"""

from __future__ import annotations

import json
import logging
import os
import re
import resource
import signal
import stat
import subprocess
import sys
import threading
import time
from collections.abc import Iterator, Sequence
from contextlib import contextmanager, ExitStack
from pathlib import Path
from typing import Any, cast, Mapping

from eden.fs.cli import mtab
from eden.integration.correctness.exploratory.core.history import (
    DAEMON_EXIT,
    Event,
    HistorySink,
    INFO,
)
from eden.integration.correctness.exploratory.core.processes import alive, poll_until
from eden.integration.correctness.exploratory.core.target import (
    EnvironmentDegraded,
    ProcessRegistry,
)
from eden.integration.correctness.exploratory.edenfs.encoding import (
    encode_committed,
    initial_content,
    SCATTER_PAYLOAD,
)
from eden.integration.correctness.exploratory.edenfs.fixture import CHURN_FILES
from eden.integration.correctness.exploratory.edenfs.reference import (
    InitialState,
    REFERENCE_DIR,
    ReferenceCheckout,
    ReferenceRuntime,
)
from eden.integration.correctness.repo import (
    AuthoredCommit,
    CommittedImages,
    create_backing_repo,
    GeneratedRepo,
    RepoAuthor,
    sapling_repository,
    system_config_path,
    TypeChange,
    WideDir,
)
from eden.integration.lib import edenclient, hgrepo
from eden.test_support.environment_variable import setenv_scope
from eden.test_support.temporary_directory import TempFileManager


logger: logging.Logger = logging.getLogger(__name__)

# Tracked, committed files with fixed content that RepoAuthor transitions
# never modify: every revision holds them byte-identical, so a normal
# checkout keeps their workload dirt.
CHURN = CHURN_FILES


def register_images(seed: int, registers: Sequence[str]) -> CommittedImages:
    """Return the revision-specific committed images of every spec register."""
    return CommittedImages(
        paths=tuple(registers),
        content=lambda path, revision: encode_committed(seed, path, revision),
    )


def _memory_cgroup_for_pid(pid: int) -> Path:
    """Return the unified cgroup directory containing ``pid``."""
    for line in Path(f"/proc/{pid}/cgroup").read_text(encoding="utf-8").splitlines():
        hierarchy, controllers, relative = line.split(":", 2)
        if hierarchy == "0" and not controllers:
            return Path("/sys/fs/cgroup") / relative.removeprefix("/")
    raise RuntimeError(f"process {pid} has no unified cgroup")


TARGET_SETUP = "target-setup"
_KILL_WAIT_SECONDS = 5.0
_FORCED_TERMINATION_SIGNAL = signal.SIGKILL
_CRASH_SIGNATURE = re.compile(
    r"EDEN_BUG|Check failed|terminate called|Segmentation|\*\*\* Signal \d+"
)
_SIGNAL_BANNER = re.compile(r"^\*\*\* Signal \d+ \((SIG[A-Z0-9]+)\)")
_LOG_TAIL_LINES = 50
# `EdenMount::co_diff` refuses a status for a commit other than the parent,
# and any status while a checkout is in progress or interrupted, only under
# `hg:enforce-parents`; the status and checkout contracts depend on both.
_EDENFS_CONFIG = {
    "hg": ["enforce-parents = true"],
}


class Environment:
    """One isolated daemon, backing repo, and clone for exploratory tests."""

    def __init__(
        self,
        seed: int = 1,
        file_count: int = 32,
        commit_count: int = 4,
        *,
        registers: Sequence[str],
        case_sensitive: bool = True,
        materialize: bool = False,
        scatter_dirty: int = 0,
        memory_max_mb: int | None = None,
        warm_readdir: bool = False,
        aged_overlay: int = 0,
        wide_dir: Mapping[str, Any] | None = None,
        type_changes: Sequence[Mapping[str, Any]] = (),
        blob_size: int | None = None,
        reference_clone: bool = False,
        run_dir: Path | None = None,
        sink: HistorySink,
    ) -> None:
        self._sink = sink
        self._reference_clone = reference_clone
        self._reference: ReferenceCheckout | None = None
        self._reference_runtime: ReferenceRuntime | None = None
        self._owned_exit = threading.Event()
        self._registers = tuple(registers)
        self._layout = GeneratedRepo(
            seed,
            file_count,
            commit_count,
            wide_dir=None if wide_dir is None else WideDir(**wide_dir),
            type_changes=tuple(TypeChange(**change) for change in type_changes),
            blob_size=blob_size,
        )
        self._case_sensitive = case_sensitive
        self._materialize = materialize
        self._scatter_dirty = scatter_dirty
        self._memory_max_mb = memory_max_mb
        self._warm_readdir_enabled = warm_readdir
        self._aged_overlay = aged_overlay
        self._setup_facts: dict[str, Any] = {}
        self._memory_cgroup: Path | None = None
        self._temp_mgr = TempFileManager()
        self._temp_mgr.set_tmp_prefix("eden_exploratory.")
        # A supervisor-owned run dir scopes every temp path (and cleanup) under it.
        self._temp_mgr._temp_dir = run_dir
        self._environment = ExitStack()
        self._eden: edenclient.EdenFS | None = None
        self._launcher: subprocess.Popen[str] | None = None
        self._daemon_pid: int | None = None
        self._mount_path: Path | None = None
        self._mount_repo: hgrepo.HgRepository | None = None
        self._backing_repo: hgrepo.HgRepository | None = None
        self._commits: tuple[AuthoredCommit, ...] = ()
        self._run_dir: Path | None = None
        self._processes = ProcessRegistry()

    @property
    def processes(self) -> ProcessRegistry:
        """Return the action-process registry owned by this environment."""
        return self._processes

    @property
    def mount_path(self) -> Path:
        """Return the clone mount point after setup."""
        if self._mount_path is None:
            raise RuntimeError("environment is not set up")
        return self._mount_path

    @property
    def commits(self) -> tuple[AuthoredCommit, ...]:
        """Return authored commits oldest first after setup."""
        if not self._commits:
            raise RuntimeError("environment is not set up")
        return self._commits

    @property
    def backing_repo(self) -> hgrepo.HgRepository:
        """Return the backing repo after setup (source for ``sl cat``)."""
        if self._backing_repo is None:
            raise RuntimeError("environment is not set up")
        return self._backing_repo

    @property
    def mount_repo(self) -> hgrepo.HgRepository:
        """Return the mounted clone after setup (target for checkout)."""
        if self._mount_repo is None:
            raise RuntimeError("environment is not set up")
        return self._mount_repo

    @property
    def reference(self) -> ReferenceCheckout:
        """Return the differential reference checkout after setup."""
        if self._reference is None:
            raise RuntimeError("environment has no reference checkout")
        return self._reference

    @property
    def run_dir(self) -> Path:
        """Return the owned run directory after setup."""
        if self._run_dir is None:
            raise RuntimeError("environment is not set up")
        return self._run_dir

    @property
    def daemon_pid(self) -> int | None:
        """Return the cached daemon pid, if one was recorded."""
        return self._daemon_pid

    @property
    def setup_facts(self) -> Mapping[str, Any]:
        """Return completed target-owned setup measurements, memory refreshed."""
        if self._memory_cgroup is not None:
            try:
                self._record_memory_facts()
            except OSError:
                logger.warning("memory facts unreadable", exc_info=True)
        return self._setup_facts

    @contextmanager
    def get_thrift_client(self, timeout: float | None = None) -> Iterator[object]:
        """Yield a thrift client bound to the owned daemon."""
        eden = self._eden
        if eden is None:
            raise RuntimeError("environment is not set up")
        with eden.get_thrift_client(timeout=timeout) as client:
            yield client

    def setup(self) -> None:
        """Start an isolated daemon, author the fixture repo, and clone it."""
        if self._eden is not None:
            raise RuntimeError("environment is already set up")

        run_dir = self._temp_mgr.make_temp_dir(prefix="run.")
        self._run_dir = run_dir
        self._setup_facts["run_dir"] = str(run_dir)
        scratch_dir = run_dir / "scratch"
        scratch_dir.mkdir()
        home_dir = run_dir / "home"
        home_dir.mkdir()
        self._environment.enter_context(setenv_scope("INTEGRATION_TEST", "1"))
        self._environment.enter_context(
            setenv_scope("SCRATCH_CONFIG_PATH", str(scratch_dir))
        )
        self._environment.enter_context(setenv_scope("HOME", str(home_dir)))

        repos_dir = run_dir / "repos"
        repos_dir.mkdir()
        repo = create_backing_repo(repos_dir, self._temp_mgr)
        # Churn content lands in rev0 via the pending add below; RepoAuthor
        # writes every register's image for each revision itself.
        for path in CHURN:
            repo.write_file(path, initial_content(self._layout.seed, path), add=False)
        repo.hg("add", *CHURN)
        images = register_images(self._layout.seed, self._registers)
        self._commits = RepoAuthor(self._layout, committed=images).author(repo)
        self._backing_repo = repo

        extra_args = self._daemon_extra_args()
        command_prefix = self._daemon_command_prefix()
        eden = edenclient.EdenFS(
            base_dir=run_dir,
            home_dir=home_dir,
            extra_args=extra_args,
            command_prefix=command_prefix,
        )
        _write_eden_config(eden.system_rc_path)
        eden.system_rollout_path.write_text(json.dumps({}) + "\n", encoding="utf-8")

        self._eden = eden
        try:
            _raise_core_limit()
            eden.start()
            process = eden._process
            if process is not None:
                self._launcher = cast(subprocess.Popen[str], process)
            self._start_exit_monitor()
            self._daemon_pid = eden.get_pid_via_thrift()
            self._configure_daemon_logging()
            self._memory_cgroup = _memory_cgroup_for_pid(self._daemon_pid)
            self._record_memory_facts()

            mounts_dir = run_dir / "mounts"
            mounts_dir.mkdir()
            self._mount_path = mounts_dir / "repo"
            eden.clone(
                repo.path,
                self._mount_path,
                case_sensitive=self._case_sensitive,
            )
            mount_repo = sapling_repository(
                self._mount_path, system_config_path(repos_dir), self._temp_mgr
            )
            self._mount_repo = mount_repo
            head = mount_repo.get_head_hash()
            if all(commit.node != head for commit in self._commits):
                raise RuntimeError(f"checkout is on unauthored commit {head[:12]}")
            self._prepare_working_copy()
            state = InitialState(head, self.initial_dirt())
            if self._reference_clone:
                self._prepare_reference(repo, state)
            self._record_memory_facts()
        except Exception as error:
            process = eden._process
            if process is not None:
                self._launcher = cast(subprocess.Popen[str], process)
            self._raise_if_setup_oom(error)
            raise
        self._sink.emit(
            Event(
                type=TARGET_SETUP,
                time=time.monotonic(),
                data={"setup": json.loads(json.dumps(self._setup_facts))},
            )
        )
        print(
            "live-ready commits=" + " ".join(commit.node for commit in self._commits),
            file=sys.stderr,
            flush=True,
        )

    def initial_dirt(self) -> tuple[tuple[str, bytes], ...]:
        """Return the modeled starting dirt: each scattered path and the bytes it holds.

        Setup writes exactly these images into the mount, and the reference
        checkout replays the same ones.
        """
        tip = len(self.commits) - 1
        return tuple(
            (self._layout.file_at(index, tip).path, SCATTER_PAYLOAD)
            for index in range(self._scatter_dirty)
        )

    def _prepare_reference(
        self, backing: hgrepo.HgRepository, state: InitialState
    ) -> None:
        """Clone the reference outside the mount and replay the mount's modeled state."""
        mount_repo = self.mount_repo
        reference = ReferenceCheckout(
            self.run_dir / REFERENCE_DIR,
            mount_repo.hg_bin,
            mount_repo.hg_environment,
            tuple(commit.node for commit in self.commits),
        )
        self._reference = reference
        self._reference_runtime = reference.runtime
        self._setup_facts["reference"] = reference.prepare(str(backing.path), state)

    def _start_exit_monitor(self) -> None:
        """Report the launcher's exit as a daemon-exit info event unless owned.

        The launcher (``edenfsctl daemon --foreground``) execs ``edenfs`` in
        place, so a fatal signal is the negative return code; otherwise the
        signal is recovered from folly's fatal-signal banner in
        ``log_tail`` when present. ``abort`` and close mark the exit owned.
        Exit timestamps precede cleanup.
        """
        launcher = self._launcher
        if launcher is None:
            raise RuntimeError("edenfs launcher was not recorded")
        owned = self._owned_exit = threading.Event()

        def _monitor() -> None:
            returncode = launcher.wait()
            exited_at = time.monotonic()
            if owned.is_set():
                return
            self._sink.emit(
                Event(type=INFO, time=exited_at, data=_exit_report(returncode))
            )

        threading.Thread(target=_monitor, daemon=True, name="daemon-exit").start()

    def _daemon_extra_args(self) -> list[str]:
        """Return daemon flags, recording the supplied ``fatal_eden_bug`` fact.

        ``EDENFS_DAEMON_FATAL_EDEN_BUG`` holds JSON boolean text; absent
        means false. The flag is passed only when the fact is true.
        """
        fatal_eden_bug = json.loads(
            os.environ.get("EDENFS_DAEMON_FATAL_EDEN_BUG", "false")
        )
        if type(fatal_eden_bug) is not bool:
            raise RuntimeError("EDENFS_DAEMON_FATAL_EDEN_BUG must be true or false")
        self._setup_facts["fatal_eden_bug"] = fatal_eden_bug
        args = ["--enable_fault_injection"]
        if fatal_eden_bug:
            args.append("--fatal_eden_bug")
        return args

    def _configure_daemon_logging(self) -> None:
        """Keep daemon logs flushed for unexpected-crash reports."""
        with self.get_thrift_client() as client:
            cast(Any, client).setOption(
                "logging", "eden.common.utils.FaultInjector=DBG1;default:async=false"
            )

    def _daemon_command_prefix(self) -> list[str]:
        """Return the daemon's command prefix: die with the spawning thread, in a transient user scope.

        ``setpriv`` sets PR_SET_PDEATHSIG(SIGKILL), then the shell execs the
        rest only while its parent is still this process, so a runner that
        died before the setting took effect leaves nothing running.
        ``systemd-run --scope`` and ``edenfsctl daemon --foreground`` exec in
        place, so the setting reaches edenfs, and the privhelper unmounts
        when edenfs dies. The signal fires when the thread that spawned the
        launcher exits: ``setup`` runs on the runner's main thread, which
        lives until the runner exits. A setuid exec clears the setting, so a
        daemon started through a sudo privhelper wrapper is not covered.
        """
        prefix = [
            "setpriv",
            "--pdeathsig",
            "KILL",
            "--",
            "sh",
            "-c",
            '[ "$PPID" = "$0" ] && exec "$@"',
            str(os.getpid()),
            "systemd-run",
            "--user",
            "--scope",
            "--quiet",
        ]
        if self._memory_max_mb is not None:
            prefix.extend(
                [
                    "-p",
                    f"MemoryMax={self._memory_max_mb}M",
                    "-p",
                    "MemorySwapMax=0",
                ]
            )
        return prefix

    def _record_memory_facts(self) -> None:
        """Refresh cgroup memory values while the daemon scope exists."""
        cgroup = self._memory_cgroup
        if cgroup is None:
            return
        max_value = (cgroup / "memory.max").read_text(encoding="utf-8").strip()
        events: dict[str, int] = {}
        for line in (cgroup / "memory.events").read_text(encoding="utf-8").splitlines():
            key, value = line.split()
            events[key] = int(value)
        self._setup_facts["memory"] = {
            "max_bytes": max_value if max_value == "max" else int(max_value),
            "current_bytes": int(
                (cgroup / "memory.current").read_text(encoding="utf-8").strip()
            ),
            "peak_bytes": int(
                (cgroup / "memory.peak").read_text(encoding="utf-8").strip()
            ),
            "events": events,
        }

    def _raise_if_setup_oom(self, error: Exception) -> None:
        """Convert a setup-time cgroup OOM kill into target-neutral data."""
        if self._memory_cgroup is None:
            for pid in (
                self._daemon_pid,
                self._launcher.pid if self._launcher is not None else None,
            ):
                if pid is None:
                    continue
                try:
                    self._memory_cgroup = _memory_cgroup_for_pid(pid)
                    break
                except (OSError, RuntimeError):
                    continue
        try:
            self._record_memory_facts()
        except OSError:
            if _scope_result(self._memory_cgroup) == "oom-kill":
                raise EnvironmentDegraded(
                    "daemon OOM-killed during setup", self._setup_facts
                ) from error
            return
        memory = self._setup_facts.get("memory")
        if isinstance(memory, dict):
            events = memory.get("events")
            if isinstance(events, dict) and events.get("oom_kill", 0) > 0:
                raise EnvironmentDegraded(
                    "daemon OOM-killed during setup",
                    self._setup_facts,
                ) from error

    def _materialize_working_copy(self) -> None:
        """Read every working-copy byte through one tar sweep."""
        try:
            sweep = subprocess.Popen(
                ["tar", "-cf", "-", "-C", str(self.mount_path), "."],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
        except OSError as error:
            raise RuntimeError("failed to start materialization sweep") from error
        if sweep.wait() != 0:
            raise RuntimeError("materialization sweep failed")

    def _prepare_working_copy(self) -> None:
        """Apply aging first and directory warming after every other setup step."""
        self._age_overlay()
        if self._materialize:
            started_at = time.monotonic()
            self._materialize_working_copy()
            self._setup_facts["materialize_ms"] = int(
                (time.monotonic() - started_at) * 1000
            )
        self._scatter_working_copy()
        if self._warm_readdir_enabled:
            started_at = time.monotonic()
            self._warm_readdir()
            self._setup_facts["warm_readdir_ms"] = int(
                (time.monotonic() - started_at) * 1000
            )

    def _age_overlay(self) -> None:
        """Visit distinct older revisions and restore the original tip each time."""
        completed = 0
        self._setup_facts["aged_overlay_round_trips"] = completed
        if not self._aged_overlay:
            return
        tip = self.commits[-1].node
        for commit in self.commits[: self._aged_overlay]:
            self.mount_repo.hg("goto", commit.node)
            self.mount_repo.hg("goto", tip)
            completed += 1
            self._setup_facts["aged_overlay_round_trips"] = completed
        if self.mount_repo.get_head_hash() != tip:
            raise RuntimeError("overlay aging did not restore the original tip")

    def _warm_readdir(self) -> None:
        """Populate directory entries and inode attributes throughout the tree."""
        for directory, directories, files in os.walk(self.mount_path):
            for name in (*directories, *files):
                os.stat(Path(directory) / name)

    def _scatter_working_copy(self) -> None:
        """Dirty the requested prefix of generated files at the tip revision."""
        for relative, payload in self.initial_dirt():
            path = self.mount_path / relative
            fd: int | None = None
            try:
                fd = os.open(path, os.O_WRONLY)
                written = os.pwrite(fd, payload, 0)
            except OSError as error:
                raise RuntimeError(f"failed to scatter dirt at {path}") from error
            finally:
                if fd is not None:
                    os.close(fd)
            if written != len(payload):
                raise RuntimeError(f"short scatter write {written}/{len(payload)}")

    def close(self, failed: bool = False) -> str | None:
        """Release all owned resources, returning cleanup errors as text."""
        errors: list[str] = []
        for name, cleanup in (
            ("reference", self._close_reference),
            ("edenfs", self._stop_eden),
            ("environment", self._environment.close),
            ("temporary directory", lambda: self._temp_mgr.cleanup(failed)),
        ):
            try:
                cleanup()
            except Exception as error:
                logger.exception("failed to clean up %s", name)
                errors.append(f"{name}: {error}")
        return "; ".join(errors) or None

    def _close_reference(self) -> None:
        """Reap every reference process group; the checkout lives under the run dir."""
        reference, self._reference = self._reference, None
        if reference is None:
            return
        leaks = reference.close_resources()
        if leaks:
            raise RuntimeError(f"{leaks} reference process group(s) survived SIGKILL")

    def abort(self) -> None:
        """Kill only the daemon identities owned by this environment."""
        self._owned_exit.set()
        _sigkill_owned_processes(self._daemon_pid, self._launcher)

    def _stop_eden(self) -> None:
        """Attempt normal shutdown, then SIGKILL only cached owned processes."""
        eden = self._eden
        launcher = self._launcher
        daemon_pid = self._daemon_pid
        run_dir = self._run_dir
        self._owned_exit.set()
        self._eden = None
        self._launcher = None
        self._daemon_pid = None
        self._mount_path = None
        self._mount_repo = None
        self._backing_repo = None
        self._run_dir = None

        shutdown_error: Exception | None = None
        if eden is not None:
            try:
                eden.kill(retry=True)
            except Exception as error:  # noqa: BLE001 - raised below
                shutdown_error = error

        if shutdown_error is not None or (
            launcher is not None and launcher.poll() is None
        ):
            _sigkill_owned_processes(daemon_pid, launcher)

        mount_error: Exception | None = None
        # After a failed normal shutdown the mount may outlive the SIGKILL;
        # the supervisor's run-dir cleanup unmounts it.
        if run_dir is not None and shutdown_error is None:
            try:
                _verify_no_owned_mounts(run_dir)
            except RuntimeError as error:
                mount_error = error

        if shutdown_error is not None:
            message = "normal EdenFS shutdown failed"
            if mount_error is not None:
                message = f"{message}; {mount_error}"
            raise RuntimeError(message) from shutdown_error
        if mount_error is not None:
            raise mount_error


def _raise_core_limit() -> None:
    """Let the daemon inherit the largest core size the hard limit allows."""
    _soft, hard = resource.getrlimit(resource.RLIMIT_CORE)
    resource.setrlimit(resource.RLIMIT_CORE, (hard, hard))


def _core_path() -> str | None:
    """Return the kernel core pattern when it is a plain path, else None."""
    pattern = Path("/proc/sys/kernel/core_pattern").read_text(encoding="utf-8")
    if pattern.startswith("|") or "%" in pattern:
        return None
    return os.path.abspath(pattern.strip())


def _exit_report(returncode: int) -> dict[str, Any]:
    """Describe an unowned launcher exit; a reader that raises becomes the ``error`` fact.

    Tail, signal, and core path are best-effort so the exit is always reported.
    """
    report: dict[str, Any] = {
        "reason": DAEMON_EXIT,
        "returncode": returncode,
        "signal": None,
        "log_tail": [],
        "core_path": None,
    }
    try:
        report["log_tail"] = _log_tail()
        report["signal"] = (
            signal.Signals(-returncode).name
            if returncode < 0
            else _signal_from_tail(report["log_tail"])
        )
        report["core_path"] = _core_path()
    except Exception as error:  # noqa: BLE001 - recorded in the event
        report["error"] = f"{type(error).__name__}: {error}"
    return report


def _log_tail() -> list[str]:
    """Return 50 daemon stderr lines from the last crash signature, else the end.

    In foreground mode the daemon logs only to stderr, which the runner
    has redirected to a file; a non-file stderr yields no tail.
    """
    try:
        if not stat.S_ISREG(os.fstat(sys.stderr.fileno()).st_mode):
            return []
        with open("/proc/self/fd/2", encoding="utf-8", errors="replace") as log:
            lines = log.read().splitlines()
    except (OSError, ValueError):
        return []
    hits = [index for index, line in enumerate(lines) if _CRASH_SIGNATURE.search(line)]
    start = hits[-1] if hits else max(0, len(lines) - _LOG_TAIL_LINES)
    return lines[start : start + _LOG_TAIL_LINES]


def _signal_from_tail(log_tail: list[str]) -> str | None:
    """Return the signal named by folly's fatal-signal banner in the tail, if any."""
    for line in log_tail:
        match = _SIGNAL_BANNER.match(line)
        if match is not None:
            return match.group(1)
    return None


def _write_eden_config(path: Path) -> None:
    lines: list[str] = []
    for section, settings in _EDENFS_CONFIG.items():
        lines.append(f"[{section}]")
        lines.extend(settings)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _verify_no_owned_mounts(run_dir: Path) -> None:
    """Fail cleanup if a mount leaked under the owned run dir."""
    leaks = tuple(_owned_mount_points(run_dir, mtab.new().read()))
    if leaks:
        raise RuntimeError(
            "owned mount(s) still present after daemon shutdown: "
            + ", ".join(os.fsdecode(path) for path in leaks)
        )


def _owned_mount_points(
    run_dir: Path, mounts: list[mtab.MountInfo]
) -> tuple[bytes, ...]:
    """Return mount points lexically beneath the owned state directory."""
    prefix = os.fsencode(run_dir)
    return tuple(
        mount.mount_point
        for mount in mounts
        if mount.mount_point == prefix or mount.mount_point.startswith(prefix + b"/")
    )


def _scope_result(cgroup: Path | None) -> str | None:
    """Return the daemon scope unit's systemd ``Result``, then reset its failed state.

    The unit is the cgroup directory's name. A scope whose processes all
    died is gone from the cgroup tree but stays loaded, failed, with its
    ``Result`` (``oom-kill`` after a cgroup OOM kill) until reset.
    """
    if cgroup is None:
        return None
    systemctl = ["systemctl", "--user"]
    try:
        shown = subprocess.run(
            [*systemctl, "show", cgroup.name, "-p", "Result", "--value"],
            capture_output=True,
            text=True,
            timeout=_KILL_WAIT_SECONDS,
        )
        subprocess.run(
            [*systemctl, "reset-failed", cgroup.name],
            capture_output=True,
            timeout=_KILL_WAIT_SECONDS,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return shown.stdout.strip() or None


def _sigkill_owned_processes(
    daemon_pid: int | None, launcher: subprocess.Popen[str] | None
) -> None:
    """SIGKILL cached daemon identities and reap the launcher process.

    The launcher is signaled through its ``Popen``, which skips a leader
    already reaped (the daemon-exit monitor waits on it), so a reused pid is
    never signaled; a daemon pid other than the launcher's is signaled
    directly.
    """
    launcher_pid = None if launcher is None else launcher.pid
    pids: set[int] = set()
    if daemon_pid is not None and daemon_pid != launcher_pid:
        pids.add(daemon_pid)
    unsafe_pids = sorted(pid for pid in pids if pid <= 1 or pid == os.getpid())
    safe_pids = pids - set(unsafe_pids)
    signaled_pids: set[int] = set()

    for pid in safe_pids:
        try:
            os.kill(pid, _FORCED_TERMINATION_SIGNAL)
            signaled_pids.add(pid)
        except ProcessLookupError:
            pass

    if launcher is not None:
        launcher.kill()
        try:
            launcher.wait(timeout=_KILL_WAIT_SECONDS)
        except subprocess.TimeoutExpired as error:
            raise RuntimeError("edenfs launcher survived SIGKILL") from error

    for pid in signaled_pids:
        if not poll_until(
            lambda pid=pid: not alive(pid),
            time.monotonic() + _KILL_WAIT_SECONDS,
            0.05,
        ):
            raise RuntimeError(f"process {pid} survived SIGKILL")

    if unsafe_pids:
        raise RuntimeError(f"refusing to signal unsafe pid(s) {unsafe_pids}")
