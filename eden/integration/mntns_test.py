#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Test Linux mntns (mount namespace) for Eden.

Requires passwordless sudo. Not suitable for automated test environments.

Tests a matrix of:
- child mntns propagation: [shared, slave, private]
- edenfs (and privhelper) in mntns: [child, parent]

Usage:

    # auto-builds binaries
    python3 mntns_test.py
    # explicit path
    python3 mntns_test.py --edenfsctl PATH
"""

import argparse
import concurrent.futures
import functools
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

run = functools.partial(subprocess.run, check=True, capture_output=True, text=True)


def build_binary(target, out=None):
    """Build an opt buck2 target and return the output path. Requires fbsource."""
    print(f"Building {target} ...", flush=True)
    buck_args = ["buck", "build", "@fbcode//mode/opt", "--show-full-output", target]
    if out:
        buck_args += ["--out", str(out)]
        run(buck_args)
        return str(out)
    result = run(buck_args)
    for line in result.stdout.splitlines():
        if " " in line:
            name, path = line.split(" ", 1)
            if name == target:
                assert os.path.isabs(path)
                return path
    raise RuntimeError(f"Could not find output for {target}")


def make_setuid_root(path):
    p = shlex.quote(str(path))
    run(["sudo", "bash", "-c", f"chown root:root {p} && chmod u+s {p}"])


def is_eden_mounted(mount_point) -> bool:
    try:
        return (Path(mount_point) / ".eden").exists()
    except OSError:
        return False


def _ns_cmd(propagation):
    """sudo unshare prefix: enters a mount namespace, re-exports env vars, drops to current user."""
    uid, gid = os.getuid(), os.getgid()
    # sudo clears the environment; re-inject via prefix assignments on exec
    env_prefix = " ".join(
        f"{k}={shlex.quote(v)}" for k, v in os.environ.items() if k.isidentifier()
    )
    return [
        "sudo",
        "unshare",
        "--mount",
        "--propagation",
        propagation,
        "--",
        "setpriv",
        f"--reuid={uid}",
        f"--regid={gid}",
        "--init-groups",
        "--",
        "bash",
        "-c",
        f'{env_prefix} exec "$@"',
        "--",
    ]


class EdenTestEnv:
    def __init__(self, edenfsctl: str, privhelper: str, base_dir: Path):
        self.edenfsctl = edenfsctl
        self.privhelper = privhelper
        self.base = base_dir
        self.state_dir = base_dir / "eden"
        self.etc_dir = base_dir / "etc_eden"
        self.home_dir = base_dir / "home"
        self.backing_repo = base_dir / "backing_repo"
        self.mount_point = base_dir / "mnt"

        for d in [self.state_dir, self.etc_dir, self.home_dir]:
            d.mkdir(parents=True, exist_ok=True)

    def init_backing_repo(self):
        run(
            [
                "sl",
                "init",
                "--config",
                "format.use-virtual-repo-with-size-factor=1",
                str(self.backing_repo),
            ]
        )

    def _edenfsctl_cmd(self, *args):
        return [
            self.edenfsctl,
            "--config-dir",
            str(self.state_dir),
            "--etc-eden-dir",
            str(self.etc_dir),
            "--home-dir",
            str(self.home_dir),
            *args,
        ]

    def _env(self):
        return {
            **os.environ,
            "EDENFS_PRIVHELPER_PATH": self.privhelper,
            "NOSCMLOG": "1",
        }

    def start_daemon(self, timeout=60):
        run(self._edenfsctl_cmd("start"), env=self._env())
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            r = run(self._edenfsctl_cmd("status"), env=self._env(), check=False)
            if r.returncode == 0 and "running" in r.stdout.lower():
                return
            time.sleep(0.5)
        raise RuntimeError("edenfs daemon failed to start")

    def clone(self):
        self.mount_point.mkdir(parents=True, exist_ok=True)
        run(
            self._edenfsctl_cmd(
                "clone",
                str(self.backing_repo),
                str(self.mount_point),
                "--rev",
                "virtual/main",
            ),
            env=self._env(),
        )

    def stop(self):
        run(
            self._edenfsctl_cmd("stop", "--timeout", "30"), env=self._env(), check=False
        )

    def clean(self):
        self.stop()
        subprocess.run(
            ["sudo", "umount", "-l", str(self.mount_point)],
            check=False,
            capture_output=True,
        )
        for d in [self.state_dir, self.mount_point]:
            shutil.rmtree(d, ignore_errors=True)
        self.state_dir.mkdir(parents=True, exist_ok=True)


def test_eden_in_parent(env: EdenTestEnv, propagation: str):
    """Start eden (and privhelper) from parent mntns"""
    env.start_daemon()
    try:
        env.clone()
        r = subprocess.run(
            _ns_cmd(propagation) + ["test", "-d", str(env.mount_point / ".eden")],
            check=False,
        )
        return {
            "parent_sees": is_eden_mounted(env.mount_point),
            "child_sees": r.returncode == 0,
        }
    finally:
        env.stop()


def test_eden_in_child(env: EdenTestEnv, propagation: str):
    """Start eden (and privhelper) from child mntns"""
    result_file = env.base / "child_sees"
    result_file.unlink(missing_ok=True)

    subprocess.run(
        _ns_cmd(propagation)
        + [
            sys.executable,
            __file__,
            "--_child",
            str(env.base),
            env.edenfsctl,
            env.privhelper,
        ],
        check=False,
    )

    if not result_file.exists():
        raise RuntimeError("child process failed")
    try:
        return {
            "parent_sees": is_eden_mounted(env.mount_point),
            "child_sees": result_file.read_text().strip() == "True",
        }
    finally:
        env.stop()


# -- Child entry point (internal, run inside namespace) --


def _child_eden(base_str, edenfsctl, privhelper):
    base = Path(base_str)
    env = EdenTestEnv(edenfsctl, privhelper, base)
    try:
        env.start_daemon()
        env.clone()
        (base / "child_sees").write_text(str(is_eden_mounted(env.mount_point)))
    except subprocess.CalledProcessError as e:
        print(
            f"child failed: {e}\nstdout: {e.stdout}\nstderr: {e.stderr}",
            file=sys.stderr,
        )
        (base / "child_sees").write_text(f"error: {e.stderr or e.stdout}")
    except Exception as e:
        print(f"child failed: {e}", file=sys.stderr)
        (base / "child_sees").write_text(f"error: {e}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--edenfsctl",
        help="Path to edenfsctl (auto-built if omitted)",
    )
    parser.add_argument("--keep", action="store_true", help="Keep temp dir on exit")
    # Internal: child process entry point (run inside mount namespace)
    parser.add_argument(
        "--_child",
        nargs=3,
        metavar=("BASE", "EDENFSCTL", "PRIVHELPER"),
        help=argparse.SUPPRESS,
    )
    args = parser.parse_args()

    if args._child:
        _child_eden(*args._child)
        return

    edenfsctl = args.edenfsctl or build_binary("fbcode//eden/fs/cli_rs:edenfsctl-run")
    keep = args.keep

    with tempfile.TemporaryDirectory(
        prefix="eden-mntns-test-",
        delete=not keep,
        ignore_cleanup_errors=True,
    ) as base_dir:
        base_dir = Path(base_dir)
        print(f"Testing temporary directory: {base_dir} ({keep=})")

        privhelper = base_dir / "edenfs_privhelper"
        build_binary("fbcode//eden/fs/service:edenfs_privhelper", out=privhelper)
        make_setuid_root(privhelper)

        env = EdenTestEnv(edenfsctl, str(privhelper), base_dir)
        env.init_backing_repo()

        propagations = ["shared", "slave", "private"]
        locations = ["parent", "child"]
        results = {}

        def run_one_test(prop, loc):
            test_base = base_dir / f"t-{prop}-{loc}"
            test_base.mkdir(parents=True)
            # Symlink shared backing repo so each test has an isolated env
            (test_base / "backing_repo").symlink_to(env.backing_repo)
            test_env = EdenTestEnv(edenfsctl, str(privhelper), test_base)
            try:
                test_fn = test_eden_in_parent if loc == "parent" else test_eden_in_child
                return test_fn(test_env, prop)
            except Exception as e:
                return {"parent_sees": f"error({e})", "child_sees": f"error({e})"}
            finally:
                test_env.clean()

        try:
            with concurrent.futures.ThreadPoolExecutor(max_workers=9) as pool:
                futures = {
                    pool.submit(run_one_test, prop, loc): (prop, loc)
                    for prop in propagations
                    for loc in locations
                }
                for fut in concurrent.futures.as_completed(futures):
                    prop, loc = futures[fut]
                    try:
                        results[(prop, loc)] = fut.result()
                    except Exception as e:
                        results[(prop, loc)] = {
                            "parent_sees": f"error({e})",
                            "child_sees": f"error({e})",
                        }
        finally:
            for prop in propagations:
                for loc in locations:
                    subprocess.run(
                        [
                            "sudo",
                            "umount",
                            "-l",
                            str(base_dir / f"t-{prop}-{loc}" / "mnt"),
                        ],
                        check=False,
                        capture_output=True,
                    )

        print(f"\n\n{'=' * 72}")
        print(
            f"{'Propagation':<12} {'Eden in':<10} {'Parent sees':<16} {'Child sees':<16}"
        )
        print(f"{'-' * 12} {'-' * 10} {'-' * 16} {'-' * 16}")
        for prop in propagations:
            for loc in locations:
                r = results.get((prop, loc), {})
                p = str(r.get("parent_sees", "?"))
                c = str(r.get("child_sees", "?"))
                print(f"{prop:<12} {loc:<10} {p:<16} {c:<16}")

    if args.keep:
        print(f"\nKept: {base_dir}")


if __name__ == "__main__":
    main()
