#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Test Linux mntns (mount namespace) for Eden.

Requires passwordless sudo. Not suitable for automated test environments.

Tests a matrix of:
- ns_prop:   child mount namespace propagation [shared, slave, private]
- fuse_prop: propagation of the FUSE mount's parent [shared, slave, private]
- eden_in:   which namespace runs edenfs+privhelper [parent, child]

Order of operations per test (important for correctness):
  1. Set up FUSE propagation: Set MS_SHARED | MS_SLAVE | MS_PRIVATE on
     the parent of the (to-be-created) eden mount. So the fuse mount
     will inherit it.
  2. Create a persistent child mount namespace (unshare --mount=FILE cat).
     The child inherits the mount table INCLUDING the above setup.
  3. Perform the FUSE mount (eden clone) in the designated namespace.
  4. Check visibility from BOTH namespaces via nsenter.

Step 2 must happen BEFORE step 3. If the child namespace were created
AFTER the FUSE mount, it would always see the mount (mount table copy at
unshare time), making the test trivially pass without testing propagation.

Usage:

    # auto-builds binaries
    python3 mntns_test.py
    # explicit paths
    python3 mntns_test.py --edenfsctl PATH --privhelper PATH
"""

import argparse
import concurrent.futures
import functools
import json
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


def get_mount_propagation(path, ns_cmd_prefix=None):
    """Return the propagation flags for a mount point (e.g. 'shared:123'), or None."""
    # --mountpoint: match only the exact mount point, not parent mounts
    cmd = ["findmnt", "-n", "-o", "PROPAGATION", "--mountpoint", str(path)]
    if ns_cmd_prefix:
        cmd = ns_cmd_prefix + cmd
    r = subprocess.run(cmd, check=False, capture_output=True, text=True)
    return " ".join(r.stdout.split()) or None


def setup_fuse_propagation(mnt_parent: Path, fuse_prop: str):
    """Bind-mount directory to itself and set its mount propagation.

    The FUSE mount point lives inside mnt_parent, so the FUSE mount
    inherits mnt_parent's propagation type.
    """
    mnt_parent.mkdir(parents=True, exist_ok=True)
    run(["sudo", "mount", "--bind", str(mnt_parent), str(mnt_parent)])
    run(["sudo", "mount", f"--make-{fuse_prop}", str(mnt_parent)])


def _nsenter_cmd(ns_file):
    """Command prefix: enter a persistent namespace, re-export env, drop to current user."""
    uid, gid = os.getuid(), os.getgid()
    env_prefix = " ".join(
        f"{k}={shlex.quote(v)}" for k, v in os.environ.items() if k.isidentifier()
    )
    return [
        "sudo",
        "nsenter",
        f"--mount={ns_file}",
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


class MountNamespace:
    """A persistent mount namespace for testing propagation.

    Created via ``unshare --mount=FILE --propagation=PROP cat``.
    ``cat`` blocks on stdin, giving us explicit lifetime control: closing
    the pipe lets cat exit.  The ``--mount=FILE`` flag bind-mounts the
    namespace to a file so we can ``nsenter --mount=FILE`` from other
    processes at any time.
    """

    def __init__(self, ns_file: Path, ns_prop: str):
        self.ns_file = ns_file
        self.ns_file.touch()
        self._proc = subprocess.Popen(
            [
                "sudo",
                "unshare",
                f"--mount={ns_file}",
                "--propagation",
                ns_prop,
                "--",
                "cat",
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self._wait_ready()

    def _wait_ready(self, timeout=5):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self._proc.poll() is not None:
                raise RuntimeError(f"unshare exited with code {self._proc.returncode}")
            r = subprocess.run(
                ["sudo", "nsenter", f"--mount={self.ns_file}", "--", "true"],
                check=False,
                capture_output=True,
            )
            if r.returncode == 0:
                return
            time.sleep(0.1)
        raise RuntimeError(
            f"Mount namespace at {self.ns_file} not ready after {timeout}s"
        )

    def enter_cmd(self):
        """Command prefix to run as current user inside this namespace."""
        return _nsenter_cmd(self.ns_file)

    def close(self):
        if self._proc.stdin:
            self._proc.stdin.close()
        self._proc.wait()
        subprocess.run(
            ["sudo", "umount", str(self.ns_file)],
            check=False,
            capture_output=True,
        )


class EdenTestEnv:
    def __init__(
        self,
        edenfsctl: str,
        privhelper: str,
        base_dir: Path,
        mount_point: Path | None = None,
    ):
        self.edenfsctl = edenfsctl
        self.privhelper = privhelper
        self.base = base_dir
        self.state_dir = base_dir / "eden"
        self.etc_dir = base_dir / "etc_eden"
        self.home_dir = base_dir / "home"
        self.backing_repo = base_dir / "backing_repo"
        self.mount_point = mount_point or base_dir / "mnt"

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


def test_eden_in_parent(env: EdenTestEnv, child_ns: MountNamespace):
    """FUSE mount in parent ns, check if child ns sees it."""
    env.start_daemon()
    try:
        env.clone()
        parent_prop = get_mount_propagation(env.mount_point)
        enter = child_ns.enter_cmd()
        r = subprocess.run(
            enter + ["test", "-d", str(env.mount_point / ".eden")],
            check=False,
        )
        child_prop = get_mount_propagation(env.mount_point, ns_cmd_prefix=enter)
        return {
            "parent_sees": is_eden_mounted(env.mount_point),
            "child_sees": r.returncode == 0,
            "parent_fuse_prop": parent_prop,
            "child_fuse_prop": child_prop,
        }
    finally:
        env.stop()


def test_eden_in_child(env: EdenTestEnv, child_ns: MountNamespace):
    """FUSE mount in child ns, check if parent ns sees it."""
    result_file = env.base / "child_sees"
    child_prop_file = env.base / "child_fuse_prop"
    result_file.unlink(missing_ok=True)
    child_prop_file.unlink(missing_ok=True)

    subprocess.run(
        child_ns.enter_cmd()
        + [
            sys.executable,
            __file__,
            "--_child",
            str(env.base),
            env.edenfsctl,
            env.privhelper,
            str(env.mount_point),
        ],
        check=False,
    )

    if not result_file.exists():
        raise RuntimeError("child process failed")
    try:
        return {
            "parent_sees": is_eden_mounted(env.mount_point),
            "child_sees": result_file.read_text().strip() == "True",
            "parent_fuse_prop": get_mount_propagation(env.mount_point),
            "child_fuse_prop": child_prop_file.read_text().strip()
            if child_prop_file.exists()
            else None,
        }
    finally:
        env.stop()


# -- Child entry point (internal, run inside namespace) --


def _child_eden(base_str, edenfsctl, privhelper, mount_point_str):
    base = Path(base_str)
    mount_point = Path(mount_point_str)
    env = EdenTestEnv(edenfsctl, privhelper, base, mount_point=mount_point)
    try:
        env.start_daemon()
        env.clone()
        (base / "child_sees").write_text(str(is_eden_mounted(mount_point)))
        prop = get_mount_propagation(mount_point)
        (base / "child_fuse_prop").write_text(prop or "")
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
    parser.add_argument(
        "--privhelper",
        help="Path to a setuid-root edenfs_privhelper (auto-built if omitted)",
    )
    parser.add_argument("--keep", action="store_true", help="Keep temp dir on exit")
    parser.add_argument(
        "-o", "--output", metavar="FILE", help="Save results (sees columns) to JSON"
    )
    parser.add_argument(
        "-c",
        "--compare",
        metavar="FILE",
        help="Compare results against a baseline JSON (from --output)",
    )
    # Internal: child process entry point (run inside mount namespace)
    parser.add_argument(
        "--_child",
        nargs=4,
        metavar=("BASE", "EDENFSCTL", "PRIVHELPER", "MOUNT_POINT"),
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

        if args.privhelper:
            privhelper = Path(args.privhelper)
        else:
            privhelper = base_dir / "edenfs_privhelper"
            build_binary("fbcode//eden/fs/service:edenfs_privhelper", out=privhelper)
            make_setuid_root(privhelper)

        env = EdenTestEnv(edenfsctl, str(privhelper), base_dir)
        env.init_backing_repo()

        ns_props = ["shared", "slave", "private"]
        fuse_props = ["shared", "slave", "private"]
        locations = ["parent", "child"]
        results = {}

        def run_one_test(ns, fuse, loc):
            test_base = base_dir / f"t-{ns}-{fuse}-{loc}"
            test_base.mkdir(parents=True)
            (test_base / "backing_repo").symlink_to(env.backing_repo)

            # Step 1: set up FUSE propagation on the mount's parent dir.
            mnt_parent = test_base / "mnt_parent"
            setup_fuse_propagation(mnt_parent, fuse)
            mount_point = mnt_parent / "checkout"

            # Step 2: create the child namespace BEFORE any FUSE mount.
            # It inherits mnt_parent's propagation from step 1. If we
            # created it after the FUSE mount, the child would always see
            # the mount (copied mount table), not testing propagation.
            ns_file = test_base / "child_ns"
            child_ns = MountNamespace(ns_file, ns)

            test_env = EdenTestEnv(
                edenfsctl, str(privhelper), test_base, mount_point=mount_point
            )
            try:
                # Step 3: FUSE mount in the designated namespace.
                # Step 4: check visibility from both namespaces.
                test_fn = test_eden_in_parent if loc == "parent" else test_eden_in_child
                return test_fn(test_env, child_ns)
            except Exception as e:
                return {"parent_sees": f"error({e})", "child_sees": f"error({e})"}
            finally:
                test_env.clean()
                child_ns.close()
                subprocess.run(
                    ["sudo", "umount", "-l", str(mnt_parent)],
                    check=False,
                    capture_output=True,
                )

        try:
            with concurrent.futures.ThreadPoolExecutor(max_workers=18) as pool:
                futures = {
                    pool.submit(run_one_test, ns, fuse, loc): (ns, fuse, loc)
                    for ns in ns_props
                    for fuse in fuse_props
                    for loc in locations
                }
                for fut in concurrent.futures.as_completed(futures):
                    key = futures[fut]
                    try:
                        results[key] = fut.result()
                    except Exception as e:
                        results[key] = {
                            "parent_sees": f"error({e})",
                            "child_sees": f"error({e})",
                        }
        finally:
            for ns in ns_props:
                for fuse in fuse_props:
                    for loc in locations:
                        test_dir = base_dir / f"t-{ns}-{fuse}-{loc}"
                        for path in [
                            test_dir / "mnt_parent" / "checkout",
                            test_dir / "mnt_parent",
                            test_dir / "child_ns",
                        ]:
                            subprocess.run(
                                ["sudo", "umount", "-l", str(path)],
                                check=False,
                                capture_output=True,
                            )

        setup_w = 12 + 12 + 10  # NS Prop + FUSE Prop + Eden in
        obs_w = 14 + 14 + 20 + 20 + 2  # results columns + separator padding
        print(f"\n\n{'=' * (setup_w + 6 + obs_w)}")
        print(f"{'Test setup':<{setup_w}}   | {'Test observation'}")
        print(
            f"{'NS Prop':<12} {'FUSE Prop':<12} {'Eden in':<10} | "
            f"{'Parent sees':<14} {'Child sees':<14} "
            f"{'Parent FUSE prop':<20} {'Child FUSE prop':<20}"
        )
        print(
            f"{'-' * 12} {'-' * 12} {'-' * 10} + "
            f"{'-' * 14} {'-' * 14} "
            f"{'-' * 20} {'-' * 20}"
        )
        for ns in ns_props:
            for fuse in fuse_props:
                for loc in locations:
                    r = results.get((ns, fuse, loc), {})
                    p = str(r.get("parent_sees", "?"))
                    c = str(r.get("child_sees", "?"))
                    pp = str(r.get("parent_fuse_prop", "?") or "-")
                    cp = str(r.get("child_fuse_prop", "?") or "-")
                    print(
                        f"{ns:<12} {fuse:<12} {loc:<10} | "
                        f"{p:<14} {c:<14} {pp:<20} {cp:<20}"
                    )

        # -- Serialize results for --output / --compare --
        # Key: "ns/fuse/loc", value: {parent_sees, child_sees} as bools
        serializable = {}
        for (ns, fuse, loc), r in results.items():
            key = f"{ns}/{fuse}/{loc}"
            serializable[key] = {
                "parent_sees": r.get("parent_sees"),
                "child_sees": r.get("child_sees"),
            }

        if args.output:
            Path(args.output).write_text(json.dumps(serializable, indent=2) + "\n")
            print(f"\nResults saved to {args.output}")

        if args.compare:
            baseline = json.loads(Path(args.compare).read_text())
            diffs = []
            for ns in ns_props:
                for fuse in fuse_props:
                    for loc in locations:
                        key = f"{ns}/{fuse}/{loc}"
                        old = baseline.get(key, {})
                        new = serializable.get(key, {})
                        op = old.get("parent_sees")
                        np = new.get("parent_sees")
                        oc = old.get("child_sees")
                        nc = new.get("child_sees")
                        if op != np or oc != nc:
                            # CAPS for changed values, lowercase for unchanged
                            ps = str(np).upper() if op != np else str(np)
                            cs = str(nc).upper() if oc != nc else str(nc)
                            diffs.append((ns, fuse, loc, ps, cs))
            if diffs:
                print(f"\n\nDifferences from {args.compare}:")
                print(
                    f"{'NS Prop':<12} {'FUSE Prop':<12} {'Eden in':<10} | "
                    f"{'Parent sees':<14} {'Child sees':<14}"
                )
                print(f"{'-' * 12} {'-' * 12} {'-' * 10} + {'-' * 14} {'-' * 14}")
                for ns, fuse, loc, ps, cs in diffs:
                    print(f"{ns:<12} {fuse:<12} {loc:<10} | {ps:<14} {cs:<14}")
            else:
                print(f"\nNo differences from {args.compare}")

    if args.keep:
        print(f"\nKept: {base_dir}")


if __name__ == "__main__":
    main()
