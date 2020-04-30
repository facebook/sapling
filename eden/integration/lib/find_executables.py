#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
These utilities are only expected to work if `sys.argv[0]` is an executable
being run in buck-out.
"""

import distutils.spawn
import logging
import os
import sys
from pathlib import Path
from typing import Callable, Dict, Optional, Tuple, Type

import eden.config


class cached_property(object):
    def __init__(self, find: Callable[["FindExeClass"], str]) -> None:
        self.name: Optional[str] = None
        self.find = find

    def __get__(self, instance: "FindExeClass", owner: Type["FindExeClass"]) -> str:
        name = self.name
        assert name is not None
        result = instance._cache.get(name, None)
        if result is None:
            result = self.find(instance)
            instance._cache[name] = result
        return result

    def __set_name__(self, owner: Type["FindExeClass"], name: str) -> None:
        self.name = name


class FindExeClass(object):
    _BUCK_OUT: Optional[str] = None
    _EDEN_SRC_ROOT: Optional[str] = None

    def __init__(self) -> None:
        self._cache: Dict[str, str] = {}

    def is_buck_build(self) -> bool:
        return eden.config.BUILD_FLAVOR == "Buck"

    @property
    def BUCK_OUT(self) -> str:
        buck_out = self._BUCK_OUT
        if buck_out is None:
            if not self.is_buck_build():
                raise Exception("There is no buck-out path in a non-Buck build")
            repo_root, buck_out = self._find_repo_root_and_buck_out()
            assert self._BUCK_OUT is not None
        return buck_out

    @property
    def EDEN_SRC_ROOT(self) -> str:
        src_root = self._EDEN_SRC_ROOT
        if src_root is None:
            if self.is_buck_build():
                src_root, buck_out = self._find_repo_root_and_buck_out()
                assert self._EDEN_SRC_ROOT is not None
            else:
                src_root = self._find_cmake_src_dir()
                self._EDEN_SRC_ROOT = src_root
        return src_root

    @cached_property
    def EDEN_CLI(self) -> str:
        return self._find_exe(
            "eden CLI",
            env="EDENFS_CLI_PATH",
            buck_path="eden/fs/cli/edenfsctl.par",
            cmake_path="eden/fs/cli/edenfsctl",
        )

    @cached_property
    def EDEN_DAEMON(self) -> str:
        edenfs_suffix = os.environ.get("EDENFS_SUFFIX", "")
        return self._find_exe(
            "edenfs daemon",
            env="EDENFS_SERVER_PATH",
            buck_path="eden/fs/service/edenfs" + edenfs_suffix,
            cmake_path="eden/fs/edenfs",
        )

    @cached_property
    def FSATTR(self) -> str:
        return self._find_exe(
            "fsattr",
            env="EDENFS_FSATTR_BIN",
            buck_path="eden/integration/helpers/fsattr",
        )

    @cached_property
    def FAKE_EDENFS(self) -> str:
        return self._find_exe(
            "fake_edenfs",
            env="EDENFS_FAKE_EDENFS",
            buck_path="eden/integration/helpers/fake_edenfs",
        )

    @cached_property
    def FORCE_SD_BOOTED(self) -> str:
        return self._find_exe(
            "force_sd_booted",
            env="EDENFS_FORCE_SD_BOOTED_PATH",
            buck_path="eden/integration/helpers/force_sd_booted",
        )

    @cached_property
    def SYSTEMD_FB_EDENFS_SERVICE(self) -> str:
        return os.path.join(self.EDEN_SRC_ROOT, "eden/fs/service/fb-edenfs@.service")

    @cached_property
    def SYSTEMD(self) -> str:
        # systemd is installed at /usr/lib/systemd/systemd on RedHat-like distributions,
        # and /bin/systemd on Debian-like distributions.
        candidates = ["/usr/lib/systemd/systemd", "/bin/systemd"]
        for path in candidates:
            if os.access(path, os.X_OK):
                return path

        raise Exception(f"unable to find systemd: candidates checked={candidates}")

    @cached_property
    def TAKEOVER_TOOL(self) -> str:
        return self._find_exe(
            "takeover_tool",
            env="EDENFS_TAKEOVER_TOOL",
            buck_path="eden/integration/helpers/takeover_tool",
        )

    @cached_property
    def ZERO_BLOB(self) -> str:
        return self._find_exe(
            "zero_blob",
            env="EDENFS_ZERO_BLOB",
            buck_path="eden/integration/helpers/zero_blob",
        )

    @cached_property
    def DROP_PRIVS(self) -> str:
        return self._find_exe(
            "drop_privs",
            env="EDENFS_DROP_PRIVS",
            buck_path="eden/fs/fuse/privhelper/test/drop_privs",
        )

    @cached_property
    def GIT(self) -> str:
        git = distutils.spawn.find_executable(
            "git.real"
        ) or distutils.spawn.find_executable("git")
        if git is None:
            raise Exception("unable to find git binary")
        return git

    @cached_property
    def HG(self) -> str:
        hg = self._find_hg()
        logging.info("Found hg binary: %r", hg)
        return hg

    @cached_property
    def HG_REAL(self) -> str:
        hg = self._find_hg_real()
        logging.info("Found hg.real binary: %r", hg)
        return hg

    def _find_hg(self) -> str:
        hg_bin = self._find_exe_optional(
            "hg", env="EDEN_HG_BINARY", buck_path="scm/telemetry/hg/hg#binary/hg"
        )
        if hg_bin:
            return hg_bin

        hg_real_bin = distutils.spawn.find_executable("hg")
        if hg_real_bin:
            return hg_real_bin

        # Fall back to the hg.real binary
        return self.HG_REAL  # pyre-ignore[7]: T38947910

    def _find_hg_real(self) -> str:
        hg_real_bin = self._find_exe_optional(
            "hg.real", env="HG_REAL_BIN", buck_path="eden/scm/__hg__/hg.sh"
        )
        if hg_real_bin:
            return hg_real_bin

        hg_real_bin = distutils.spawn.find_executable("hg.real")
        if hg_real_bin:
            return hg_real_bin

        hg_real_bin = distutils.spawn.find_executable("hg")
        if hg_real_bin:
            return hg_real_bin

        raise Exception("No hg binary found!")

    def _find_exe(
        self,
        name: str,
        env: str,
        buck_path: Optional[str] = None,
        cmake_path: Optional[str] = None,
    ) -> str:
        exe = self._find_exe_optional(
            name=name,
            env=env,
            buck_path=buck_path,
            cmake_path=cmake_path,
            require_found=True,
        )
        assert exe is not None
        return exe

    def _find_exe_optional(
        self,
        name: str,
        env: str,
        buck_path: Optional[str] = None,
        cmake_path: Optional[str] = None,
        require_found: bool = False,
    ) -> Optional[str]:
        if env is not None:
            path = os.environ.get(env)
            if path:
                if not os.access(path, os.X_OK):
                    raise Exception(
                        f"unable to find {name}: specified as {path!r} "
                        f"by ${env}, but not available there"
                    )
                return path

        candidates = []
        if self.is_buck_build():
            if buck_path is not None:
                candidates.append(os.path.join(self.BUCK_OUT, "gen", buck_path))
        else:
            # If an explicit CMake output was specified use it,
            # otherwise use the same path as for Buck.
            if cmake_path is not None:
                candidates.append(os.path.join(os.getcwd(), cmake_path))
            elif buck_path is not None:
                candidates.append(os.path.join(os.getcwd(), buck_path))

        if sys.platform == "win32":
            # All of the paths in the candidates list are POSIX-style paths.
            # Convert path separators to Windows-style.
            # Also search for both the path as specified, as well as with a ".exe"
            # suffix, since the CMake build automatically adds a .exe suffix all
            # binaries.
            win_candidates = []
            for path in candidates:
                win_path = path.replace("/", os.path.sep)
                win_candidates.append(win_path)
                win_candidates.append(win_path + ".exe")
            candidates = win_candidates

        for path in candidates:
            if os.access(path, os.X_OK):
                return path

        if require_found:
            raise Exception(f"unable to find {name}: candidates checked={candidates}")

        return None

    def _find_repo_root_and_buck_out(self) -> Tuple[str, str]:
        """Finds the paths to the repo root and the buck-out directory.

        Note that the path to buck-out may not be "buck-out" under the repo
        root because Buck could have been run with `buck --config
        project.buck_out` and sys.argv[0] could be the realpath rather than the
        symlink under buck-out.
        """
        executable = sys.argv[0]
        path = os.path.dirname(os.path.abspath(executable))
        while True:
            parent = os.path.dirname(path)
            parent_basename = os.path.basename(parent)
            if parent_basename == "buck-out":
                src_root = os.path.dirname(parent)
                if os.path.basename(path) in ["bin", "gen"]:
                    buck_out = parent
                else:
                    buck_out = path
                self._EDEN_SRC_ROOT = src_root
                self._BUCK_OUT = buck_out
                return (src_root, buck_out)
            if parent == path:
                raise Exception("Path to repo root not found from %s" % executable)
            path = parent

    def _find_cmake_src_dir(self) -> str:
        src_dir = os.environ.get("CMAKE_SOURCE_DIR", "")
        if src_dir:
            return src_dir

        # The CMAKE_SOURCE_DIR environment variable should always be set if running the
        # tests through ctest.  However, if the test executable is invoked directly
        # CMAKE_SOURCE_DIR may not be set.  If this build was driven by getdeps using an
        # internal shipit-based build, the sources may exist at ../../shipit/eden.
        # Try looking at that location just to make it easier for developers to run the
        # test binary manually.
        eden_dir = Path.cwd().parent.parent / "shipit" / "eden"
        if eden_dir.exists():
            return str(eden_dir)

        raise Exception(
            "unable to find source directory: "
            "CMAKE_SOURCE_DIR environment variable is not set"
        )


# The main FindExe singleton
FindExe = FindExeClass()
