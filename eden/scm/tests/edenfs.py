# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

# This file would be much better as a debugruntest extension, but there are a
# few things preventing that:
# 1. EdenFS setup env var setup is complex and difficult to keep in sync. This
#    approach instead opts for using the exact env vars that were passed down
#    to the test and then filters the ones that might be altered on the fly
#    by tests.
# 2. Running sl clone with EdenFS depends on having `edenfsctl` as an env var
# 3. Cross-OS shenanigans related to TMPDIR, which is used extensively on debugruntest
# 4. Starting EdenFS has a few caveats:
#    a. The logic for waiting for EdenFS to start only fully exists in
#       eden.integration.lib.edenclient. In theory this could be replaced with
#       `eden status --wait`, but at least at the time this was originally written
#       that never really worked.
#    b. Adding a regular sleep after starting EdenFS is a bad idea. This makes
#       testing slower, and we don't want slowness preventing us from enabling
#       EdenFS on more .t tests.
#    c. EdenFS has to be started before the entire test can be run so that we
#       can have useful logs when there is an error. This is mostly a choice
#       rather than a real requirement, however.
# 5. It's better to make sure EdenFS is completely terminated once a test
#    finishes. Not killing EdenFS can lead to mount shenanigans in some OSes

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
                # pyre-fixme[16]: `EdenFsManager` has no attribute `eden`.
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
                    elif sys.platform == "win32":
                        eden_rc.write(
                            """
[notifications]
enable-eden-menu = "false"
"""
                        )

                if not "USE_MONONOKE" in os.environ:
                    # As mentioned above, not starting EdenFS is far from ideal,
                    # but Mononoke tests (ab)use env vars, and in particular
                    # rewrites HGRCPATH constantly. It puts the config for
                    # edenapi there, so we cannot really start EdenFS until
                    # that setting there is set. For Mononoke tests, EdenFS
                    # will be started there.
                    self.eden.start()
                self.generate_eden_cli_wrapper(orig_test_dir)
            except Exception as e:
                ex = e

        if ex:
            raise ex

    # pyre-fixme[3]: Return type must be annotated.
    def generate_eden_cli_wrapper(self, binpath: Path):
        # pyre-fixme[16]: `EdenFsManager` has no attribute `eden`.
        cmd, env = self.eden.get_edenfsctl_cmd_env("", config_dir=True, home_dir=False)

        edenpath = binpath.parents[1] / "install" / "bin" / "eden"
        # These two are not really necessary and contain symbols that might be
        # annoying to escape, so let's get rid of them
        env.pop("HGTEST_EXCLUDED", None)
        env.pop("HGTEST_INCLUDED", None)
        # See comment about Monooke above for these two env vars below
        env.pop("HGRCPATH", None)
        env.pop("SL_CONFIG_PATH", None)
        # .t tests set the value for $HOME to $TESTTMP, and we don't want to
        # force every EdenFS command to have the current value of $HOME at this
        # point (which will likely be different to $TESTTMP). The $HOME in the
        # generated scripts below will be $TESTTMP at runtime.
        env.pop("HOME", None)
        if not os.name == "nt":
            with open(edenpath, "w") as f:
                f.write("#!/usr/bin/env bash\n")
                for k, v in env.items():
                    f.write(f"export {k}={repr(v)}\n")
                f.write(" ".join(cmd) + '--home-dir "$HOME" "$@"\n')
            os.chmod(edenpath, 0o775)
        else:
            with open(str(edenpath) + ".bat", "w") as f:
                f.write("@echo off\n")
                for k, v in env.items():
                    f.write(f"set {k}={v}\n")
                cmd[0] = f'"{cmd[0]}"'
                fullpath = (" ".join(cmd)).strip()
                f.write(f"{fullpath} --home-dir %HOME% %*\n")
                f.write("exit /B %errorlevel%\n")
