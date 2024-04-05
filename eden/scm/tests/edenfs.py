# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import sys
from contextlib import contextmanager
from pathlib import Path

from typing import Dict, Generator

from eden.integration.lib import edenclient


@contextmanager
def override_environ(values: Dict[str, str]) -> Generator[None, None, None]:
    backup = {}
    for key, value in values.items():
        old = os.environ.get(key, None)
        if old is not None:
            backup[key] = old
        os.environ[key] = value
    yield
    for key in values.keys():
        os.environ.pop(key, None)
        old = backup.get(key, None)
        if old is not None:
            os.environ[key] = old


class EdenFsManager:
    test_dir: Path

    def __init__(self, test_dir: Path) -> None:
        self.test_dir = test_dir

    def start(self, overrides: Dict[str, str]) -> None:
        overrides = dict(overrides)
        orig_test_dir = self.test_dir

        # Valid socketpaths on macOS are too short, and the usual temp dir created when running
        # this with Buck is too large. Similar to what's done in D43988120
        if sys.platform == "darwin":
            self.test_dir = Path(
                os.path.join(
                    "/tmp/", orig_test_dir.parts[-3], orig_test_dir.parts[-2], "eden"
                )
            )
        else:
            self.test_dir = orig_test_dir

        os.makedirs(self.test_dir)

        edenfs_dir = self.test_dir / "eden_test_config"
        os.mkdir(edenfs_dir)

        scratch_config = self.test_dir / "eden_scratch_config"
        with open(scratch_config, "w+") as f:
            template_dir = self.test_dir / "template_dir"
            os.mkdir(template_dir)
            template_dir = repr(str(template_dir))[1:-1]
            f.write(
                f"""
template = "{template_dir}"
overrides = "{{}}"
"""
            )

        overrides.update(
            {
                "SCRATCH_CONFIG_PATH": str(scratch_config),
                "INTEGRATION_TEST": "1",
                "HOME": str(self.test_dir),
                # Just in case
                "CHGDISABLE": "1",
            }
        )

        # See D43988120 to see why this is necessary
        if sys.platform == "darwin":
            overrides["TMPDIR"] = "/tmp"

        ex = None
        with override_environ(overrides):
            try:
                self.eden = edenclient.EdenFS(
                    base_dir=edenfs_dir,
                    storage_engine="memory",
                )

                # Write out edenfs config file.
                with open(self.eden.system_rc_path, mode="w") as eden_rc:
                    eden_rc.write(
                        """
[experimental]
enable-nfs-server = "true"
"""
                    )

                    if sys.platform == "darwin":
                        eden_rc.write(
                            """
[clone]
default-mount-protocol = "NFS"

[nfs]
allow-apple-double = "false"
"""
                        )
                        if "SANDCASTLE" in os.environ:
                            eden_rc.write(
                                """
[redirections]
darwin-redirection-type = "symlink"
"""
                            )

                self.eden.start()
                self.generate_eden_cli_wrapper(orig_test_dir)
            except Exception as e:
                ex = e

        if ex:
            raise ex

    def generate_eden_cli_wrapper(self, binpath: Path):
        cmd, env = self.eden.get_edenfsctl_cmd_env("", config_dir=True)
        edenpath = binpath.parents[1] / "install" / "bin" / "eden"
        # These two are not really necessary and contain symbols that might be
        # annoying to escape, so let's get rid of them
        env.pop("HGTEST_EXCLUDED", None)
        env.pop("HGTEST_INCLUDED", None)
        if not os.name == "nt":
            with open(edenpath, "w") as f:
                f.write("#!/usr/bin/env bash\n")
                for k, v in env.items():
                    f.write(f"export {k}={repr(v)}\n")
                f.write(" ".join(cmd) + ' "$@"\n')
            os.chmod(edenpath, 0o775)
        else:
            with open(str(edenpath) + ".bat", "w") as f:
                f.write("@echo off\n")
                for k, v in env.items():
                    f.write(f"set {k}={v}\n")
                cmd[0] = f'"{cmd[0]}"'
                fullpath = (" ".join(cmd)).strip()
                f.write(f"{fullpath} %*\n")
                f.write("exit /B %errorlevel%\n")
