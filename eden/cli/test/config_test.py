#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import configparser
import io
import os
import unittest
from typing import Dict, List, Optional

import toml
import toml.decoder
from eden.test_support.environment_variable import EnvironmentVariableMixin
from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from .. import config as config_mod, configutil, util
from ..config import EdenInstance


def get_toml_test_file_invalid():
    cfg_file = """
[core thisIsNotAllowed]
"""
    return cfg_file


def get_toml_test_file_defaults():
    cfg_file = """
[core]
systemIgnoreFile = "/etc/eden/gitignore"
ignoreFile = "/home/${USER}/.gitignore"

[clone]
default-revision = "master"

[rage]
reporter = 'arc paste --title "eden rage from $(hostname)" --conduit-uri=https://phabricator.intern.facebook.com/api/'
"""
    return cfg_file


def get_toml_test_file_hooks():
    cfg_file = """
[hooks]
"hg.edenextension" = "/usr/local/fb-mercurial/eden/hgext3rd/eden"
"""
    return cfg_file


def get_toml_test_file_fbsource_repo():
    cfg_file = """
["repository fbsource"]
type = "hg"
path = "/data/users/${USER}/fbsource"

["bindmounts fbsource"]
fbcode-buck-out = "fbcode/buck-out"
buck-out = "buck-out"
"""
    return cfg_file


def get_toml_test_file_user_rc():
    cfg_file = """
[core]
ignoreFile = "/home/${USER}/.gitignore-override"
edenDirectory = "/home/${USER}/.eden"

["repository fbsource"]
type = "hg"
path = "/data/users/${USER}/fbsource-override"

["bindmounts fbsource"]
fbcode-buck-out = "fbcode/buck-out-override"

["repository git"]
type = "git"
path = "/home/${USER}/src/git/.git"
hooks = "/home/${USER}/my-git-hook"
"""
    return cfg_file


class TomlConfigTest(
    unittest.TestCase, TemporaryDirectoryMixin, EnvironmentVariableMixin
):
    def setUp(self) -> None:
        self._test_dir = self.make_temporary_directory()

        self._user = "bob"
        self._state_dir = os.path.join(self._test_dir, ".eden")
        self._etc_eden_dir = os.path.join(self._test_dir, "etc/eden")
        self._config_d = os.path.join(self._test_dir, "etc/eden/config.d")
        self._home_dir = os.path.join(self._test_dir, "home", self._user)
        self._interpolate_dict = {"USER": self._user, "HOME": self._home_dir}

        os.mkdir(self._state_dir)
        util.mkdir_p(self._config_d)
        util.mkdir_p(self._home_dir)

        self.unset_environment_variable("EDEN_EXPERIMENTAL_SYSTEMD")

    def copy_config_files(self) -> None:
        path = os.path.join(self._config_d, "defaults.toml")
        with open(path, "w") as text_file:
            text_file.write(get_toml_test_file_defaults())

        path = os.path.join(self._config_d, "fbsource.repo.toml")
        with open(path, "w") as text_file:
            text_file.write(get_toml_test_file_fbsource_repo())

        path = os.path.join(self._config_d, "hooks.toml")
        with open(path, "w") as text_file:
            text_file.write(get_toml_test_file_hooks())

        path = os.path.join(self._home_dir, ".edenrc")
        with open(path, "w") as text_file:
            text_file.write(get_toml_test_file_user_rc())

    def assert_core_config(self, cfg: EdenInstance) -> None:
        self.assertEqual(
            cfg.get_config_value("rage.reporter"),
            'arc paste --title "eden rage from $(hostname)" --conduit-uri=https://phabricator.intern.facebook.com/api/',
        )
        self.assertEqual(
            cfg.get_config_value("core.ignoreFile"),
            f"/home/{self._user}/.gitignore-override",
        )
        self.assertEqual(
            cfg.get_config_value("core.systemIgnoreFile"), "/etc/eden/gitignore"
        )
        self.assertEqual(
            cfg.get_config_value("core.edenDirectory"), f"/home/{self._user}/.eden"
        )

    def assert_git_repo_config(self, cfg: EdenInstance) -> None:
        cc = cfg.find_config_for_alias("git")
        assert cc is not None
        self.assertEqual(cc.path, f"/home/{self._user}/src/git/.git")
        self.assertEqual(cc.scm_type, "git")
        self.assertEqual(cc.hooks_path, f"/home/{self._user}/my-git-hook")
        self.assertEqual(cc.bind_mounts, {})
        self.assertEqual(cc.default_revision, "master")

    def assert_fbsource_repo_config(self, cfg: EdenInstance) -> None:
        cc = cfg.find_config_for_alias("fbsource")
        assert cc is not None
        self.assertEqual(cc.path, f"/data/users/{self._user}/fbsource-override")
        self.assertEqual(cc.scm_type, "hg")
        self.assertEqual(
            cc.bind_mounts,
            {"fbcode-buck-out": "fbcode/buck-out-override", "buck-out": "buck-out"},
        )
        self.assertEqual(cc.default_revision, "master")

    def test_load_config(self) -> None:
        self.copy_config_files()
        cfg = self.get_config()

        # Check the various config sections
        self.assert_core_config(cfg)
        exp_repos = ["fbsource", "git"]
        self.assertEqual(cfg.get_repository_list(), exp_repos)
        self.assert_fbsource_repo_config(cfg)
        self.assert_git_repo_config(cfg)

        # Check if test is for toml or cfg by cfg._user_toml_cfg
        exp_rc_files = [
            os.path.join(self._config_d, "defaults.toml"),
            os.path.join(self._config_d, "fbsource.repo.toml"),
            os.path.join(self._config_d, "hooks.toml"),
            os.path.join(self._home_dir, ".edenrc"),
        ]
        self.assertEqual(cfg.get_rc_files(), exp_rc_files)

    def test_no_dot_edenrc(self) -> None:
        self.copy_config_files()

        os.remove(os.path.join(self._home_dir, ".edenrc"))
        cfg = self.get_config()
        cfg._loadConfig()

        exp_repos = ["fbsource"]
        self.assertEqual(cfg.get_repository_list(), exp_repos)

        self.assertEqual(
            cfg.get_config_value("rage.reporter"),
            'arc paste --title "eden rage from $(hostname)" --conduit-uri=https://phabricator.intern.facebook.com/api/',
        )
        self.assertEqual(
            cfg.get_config_value("core.ignoreFile"), f"/home/{self._user}/.gitignore"
        )
        self.assertEqual(
            cfg.get_config_value("core.systemIgnoreFile"), "/etc/eden/gitignore"
        )
        cc = cfg.find_config_for_alias("fbsource")
        assert cc is not None
        self.assertEqual(cc.path, f"/data/users/{self._user}/fbsource")
        self.assertEqual(cc.scm_type, "hg")
        self.assertEqual(
            cc.bind_mounts,
            {"fbcode-buck-out": "fbcode/buck-out", "buck-out": "buck-out"},
        )
        self.assertEqual(cc.default_revision, "master")

    def test_add_existing_repo(self) -> None:
        self.copy_config_files()

        cfg = self.get_config()
        with self.assertRaisesRegex(
            config_mod.UsageError,
            "repository fbsource already exists. You will need to edit "
            "the ~/.edenrc config file by hand to make changes to the "
            "repository or remove it.",
        ):
            cfg.add_repository("fbsource", "hg", f"/data/users/{self._user}/fbsource")

    def test_add_repo(self) -> None:
        self.copy_config_files()

        cfg = self.get_config()
        cfg.add_repository("fbandroid", "hg", f"/data/users/{self._user}/fbandroid")

        # Lets reload our config
        cfg = self.get_config()
        # Check the various config sections
        self.assert_core_config(cfg)
        exp_repos = ["fbandroid", "fbsource", "git"]
        self.assertEqual(cfg.get_repository_list(), exp_repos)
        self.assert_fbsource_repo_config(cfg)
        self.assert_git_repo_config(cfg)

        # Check the newly added repo
        cc = cfg.find_config_for_alias("fbandroid")
        assert cc is not None
        self.assertEqual(cc.path, f"/data/users/{self._user}/fbandroid")
        self.assertEqual(cc.scm_type, "hg")
        self.assertEqual(cc.hooks_path, f"{self._etc_eden_dir}/hooks")
        self.assertEqual(cc.bind_mounts, {})
        self.assertEqual(cc.default_revision, "master")

    def test_toml_error(self) -> None:
        self.copy_config_files()

        self.write_user_config(get_toml_test_file_invalid())

        cfg = self.get_config()
        with self.assertRaises(toml.decoder.TomlDecodeError):
            cfg._loadConfig()

    def test_experimental_systemd_is_disabled_by_default(self) -> None:
        self.assertFalse(self.get_config().should_use_experimental_systemd_mode())

    def test_experimental_systemd_is_enabled_with_environment_variable(self) -> None:
        self.set_environment_variable("EDEN_EXPERIMENTAL_SYSTEMD", "1")
        self.assertTrue(self.get_config().should_use_experimental_systemd_mode())

    def test_experimental_systemd_is_enabled_with_user_config_setting(self) -> None:
        self.write_user_config(
            """[service]
experimental_systemd = true
"""
        )
        self.assertTrue(self.get_config().should_use_experimental_systemd_mode())

    def test_experimental_systemd_environment_variable_overrides_config(self) -> None:
        self.set_environment_variable("EDEN_EXPERIMENTAL_SYSTEMD", "1")
        self.write_user_config(
            f"""[service]
experimental_systemd = false
"""
        )
        self.assertTrue(self.get_config().should_use_experimental_systemd_mode())

        self.set_environment_variable("EDEN_EXPERIMENTAL_SYSTEMD", "0")
        self.write_user_config(
            f"""[service]
experimental_systemd = true
"""
        )
        self.assertFalse(self.get_config().should_use_experimental_systemd_mode())

    def test_empty_experimental_systemd_environment_variable_does_not_override_config(
        self
    ) -> None:
        self.set_environment_variable("EDEN_EXPERIMENTAL_SYSTEMD", "")
        self.write_user_config(
            f"""[service]
experimental_systemd = true
"""
        )
        self.assertTrue(self.get_config().should_use_experimental_systemd_mode())

        self.set_environment_variable("EDEN_EXPERIMENTAL_SYSTEMD", "")
        self.write_user_config(
            f"""[service]
experimental_systemd = false
"""
        )
        self.assertFalse(self.get_config().should_use_experimental_systemd_mode())

    def test_printed_config_is_valid_toml(self) -> None:
        self.write_user_config(
            """
[clone]
default-revision = "master"
"""
        )

        printed_config = io.StringIO()
        self.get_config().print_full_config(file=printed_config)
        printed_config.seek(0)
        parsed_config = toml.load(printed_config)

        self.assertIn("clone", parsed_config)
        self.assertEqual(parsed_config["clone"].get("default-revision"), "master")

    def test_printed_config_expands_variables(self) -> None:
        self.write_user_config(
            """
["repository fbsource"]
type = "hg"
path = "/data/users/${USER}/fbsource"
"""
        )

        printed_config = io.StringIO()
        self.get_config().print_full_config(file=printed_config)

        self.assertIn("/data/users/bob/fbsource", printed_config.getvalue())

    def get_config(self) -> EdenInstance:
        return EdenInstance(
            self._state_dir, self._etc_eden_dir, self._home_dir, self._interpolate_dict
        )

    def write_user_config(self, content: str) -> None:
        path = os.path.join(self._home_dir, ".edenrc")
        with open(path, "w") as text_file:
            text_file.write(content)
