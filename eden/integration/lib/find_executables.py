#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

"""
These utilities are only expected to work if `sys.argv[0]` is an executable
being run in buck-out.
"""

import distutils.spawn
import logging
import os
import sys
import typing
from typing import Callable, Dict, List, Optional, Type

from libfb.py import pathutils


class cached_property(object):
    def __init__(self, find: Callable[["FindExeClass"], str]) -> None:
        self.name: Optional[str] = None
        self.find = find

    def __get__(self, instance: "FindExeClass", owner: Type["FindExeClass"]) -> str:
        assert self.name is not None
        result = instance._cache.get(self.name, None)
        if result is None:
            result = self.find(instance)
            instance._cache[self.name] = result
        return result

    def __set_name__(self, owner: Type["FindExeClass"], name: str) -> None:
        self.name = name


class FindExeClass(object):
    _BUCK_OUT: Optional[str] = None
    _REPO_ROOT: Optional[str] = None

    def __init__(self) -> None:
        self._cache: Dict[str, str] = {}

    @property
    def BUCK_OUT(self) -> str:
        if self._BUCK_OUT is None:
            self._find_repo_root_and_buck_out()
            assert self._BUCK_OUT is not None
        return self._BUCK_OUT

    @property
    def REPO_ROOT(self) -> str:
        if self._REPO_ROOT is None:
            self._find_repo_root_and_buck_out()
            assert self._REPO_ROOT is not None
        return self._REPO_ROOT

    @cached_property
    def EDEN_CLI(self) -> str:
        return self._find_exe(
            "eden CLI",
            env="EDENFS_CLI_PATH",
            candidates=[os.path.join(self.BUCK_OUT, "gen/eden/cli/edenfsctl.par")],
        )

    @cached_property
    def EDEN_DAEMON(self) -> str:
        edenfs_suffix = os.environ.get("EDENFS_SUFFIX", "")
        edenfs = os.path.join(
            self.BUCK_OUT, "gen/eden/fs/service/edenfs%s" % edenfs_suffix
        )
        return self._find_exe(
            "edenfs daemon", env="EDENFS_SERVER_PATH", candidates=[edenfs]
        )

    @cached_property
    def FSATTR(self) -> str:
        return self._find_exe(
            "fsattr",
            env="EDENFS_FSATTR_BIN",
            candidates=[
                os.path.join(self.BUCK_OUT, "gen/eden/integration/helpers/fsattr")
            ],
        )

    @cached_property
    def FAKE_EDENFS(self) -> str:
        return self._find_exe(
            "fake_edenfs",
            env="EDENFS_FAKE_EDENFS",
            candidates=[
                os.path.join(self.BUCK_OUT, "gen/eden/integration/helpers/fake_edenfs")
            ],
        )

    @cached_property
    def FORCE_SD_BOOTED(self) -> str:
        return self._find_exe(
            "force_sd_booted",
            env="EDENFS_FORCE_SD_BOOTED_PATH",
            candidates=[
                os.path.join(
                    self.BUCK_OUT, "gen/eden/integration/helpers/force_sd_booted"
                )
            ],
        )

    @cached_property
    def SYSTEMD_FB_EDENFS_SERVICE(self) -> str:
        return os.path.join(self.REPO_ROOT, "eden/fs/service/fb-edenfs@.service")

    @cached_property
    def TAKEOVER_TOOL(self) -> str:
        return self._find_exe(
            "takeover_tool",
            env="EDENFS_TAKEOVER_TOOL",
            candidates=[
                os.path.join(
                    self.BUCK_OUT, "gen/eden/integration/helpers/takeover_tool"
                )
            ],
        )

    @cached_property
    def ZERO_BLOB(self) -> str:
        return self._find_exe(
            "zero_blob",
            env="EDENFS_ZERO_BLOB",
            candidates=[
                os.path.join(self.BUCK_OUT, "gen/eden/integration/helpers/zero_blob")
            ],
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
        # If EDEN_HG_BINARY is set in the environment, use that.
        # This is always set when the tests are run by `buck test`
        # The code below is only used as a fallback when the tests are invoked manually.
        hg_bin = os.environ.get("EDEN_HG_BINARY")
        if hg_bin:
            return hg_bin

        # Look for the hg wrapper
        start_path = os.path.abspath(sys.argv[0])
        hg_bin = pathutils.get_build_rule_output_path(
            "//scm/telemetry/hg:hg",
            pathutils.BuildRuleTypes.RUST_BINARY,
            start_path=start_path,
        )
        if hg_bin:
            return hg_bin

        # Fall back to the real hg binary
        return typing.cast(str, self.HG_REAL)  # T38947910

    def _find_hg_real(self) -> str:
        # If HG_REAL_BIN is set in the environment, use that.
        # This is always set when the tests are run by `buck test`
        # The code below is only used as a fallback when the tests are invoked manually.
        hg_real_bin = os.environ.get("HG_REAL_BIN")
        if hg_real_bin:
            return hg_real_bin

        start_path = os.path.abspath(sys.argv[0])
        hg_real_bin = pathutils.get_build_rule_output_path(
            "//scm/hg:hg", pathutils.BuildRuleTypes.PYTHON_BINARY, start_path=start_path
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

    def _find_exe(self, name: str, env: str, candidates: List[str]) -> str:
        if env is not None:
            path = os.environ.get(env)
            if path:
                if not os.access(path, os.X_OK):
                    raise Exception(
                        f"unable to find {name}: specified as {path!r} "
                        f"by ${env}, but not available there"
                    )
                return path

        for path in candidates:
            if os.access(path, os.X_OK):
                return path

        raise Exception(f"unable to find {name}")

    def _find_repo_root_and_buck_out(self) -> None:
        """Finds the paths to buck-out and the repo root.

        Note that the path to buck-out may not be "buck-out" under the repo
        root because Buck could have been run with `buck --config
        project.buck_out` and sys.argv[0] could be the realpath rather than the
        symlink under buck-out.

        TODO: We will have to use a different heuristic for open source builds
        that build with CMake. (Ultimately, we would prefer to build them with
        Buck.)
        """
        executable = sys.argv[0]
        path = os.path.dirname(os.path.abspath(executable))
        while True:
            parent = os.path.dirname(path)
            parent_basename = os.path.basename(parent)
            if parent_basename == "buck-out":
                self._REPO_ROOT = os.path.dirname(parent)
                if os.path.basename(path) in ["bin", "gen"]:
                    self._BUCK_OUT = parent
                else:
                    self._BUCK_OUT = path
                return
            if parent == path:
                raise Exception("Path to repo root not found from %s" % executable)
            path = parent


# The main FindExe singleton
FindExe = FindExeClass()
