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
import os
import shutil
import tempfile
import unittest
from typing import Dict, List, Optional

import toml

from .. import config, configutil, util


def get_cfg_test_file_defaults():
    cfg_file = """
[core]
systemIgnoreFile = /etc/eden/gitignore
ignoreFile = /home/${USER}/.gitignore

[clone]
default-revision = master

[rage]
reporter=arc paste --title "eden rage from $(hostname)" --conduit-uri=https://phabricator.intern.facebook.com/api/
"""
    return cfg_file


def get_cfg_test_file_hooks():
    cfg_file = """
[hooks]
hg.edenextension = /usr/local/fb-mercurial/eden/hgext3rd/eden
"""
    return cfg_file


def get_cfg_test_file_fbsource_repo():
    cfg_file = """
[repository fbsource]
type = hg
path = /data/users/${USER}/fbsource

[bindmounts fbsource]
fbcode-buck-out = fbcode/buck-out
buck-out = buck-out
"""
    return cfg_file


def get_cfg_test_file_user_rc():
    cfg_file = """
[core]
ignoreFile=/home/${USER}/.gitignore-override
edenDirectory=/home/${USER}/.eden

[repository fbsource]
type = hg
path = /data/users/${USER}/fbsource-override

[bindmounts fbsource]
fbcode-buck-out = fbcode/buck-out-override

[repository git]
type = git
path = /home/${USER}/src/git/.git
hooks = /home/${USER}/my-git-hook
"""
    return cfg_file


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


# Utility method to get config as string
def get_config_as_string(config: configparser.ConfigParser) -> str:
    s = ""
    for section in config.sections():
        s += "[" + section + "]\n"
        for k, v in config.items(section):
            s += k + "=" + v + "\n"
    return s


class ForceFileMockConfig(config.Config):
    def __init__(
        self,
        config_dir: str,
        etc_eden_dir: str,
        home_dir: str,
        rc_file_list: List[str],
        interpolate_dict: Optional[Dict[str, str]] = None,
        use_toml_cfg: bool = False,
    ) -> None:
        super().__init__(
            config_dir, etc_eden_dir, home_dir, interpolate_dict, use_toml_cfg
        )
        self._rc_file_list = rc_file_list

    def get_rc_files(self):
        return self._rc_file_list


# TopConfigTestBase is "container" for ConfigTestBase. Unittest only runs tests
# on module level classes. Thus, ConfigTestBase's tests will only be run on
# its subclasses (tidier approach than multiple inheritance)
class TopConfigTestBase(object):
    class ConfigTestBase(unittest.TestCase):
        def setUp(self):
            self._test_dir = tempfile.mkdtemp(prefix="eden_config_test.")
            self.addCleanup(shutil.rmtree, self._test_dir)

            self._user = "bob"
            self._config_dir = os.path.join(self._test_dir, ".eden")
            self._etc_eden_dir = os.path.join(self._test_dir, "etc/eden")
            self._config_d = os.path.join(self._test_dir, "etc/eden/config.d")
            self._home_dir = os.path.join(self._test_dir, "home", self._user)
            self._interpolate_dict = {"USER": self._user, "HOME": self._home_dir}

            os.mkdir(self._config_dir)
            util.mkdir_p(self._config_d)
            util.mkdir_p(self._home_dir)

        @abc.abstractmethod
        def copy_config_files(self):
            pass

        def copy_cfg_config_files(self):
            path = os.path.join(self._config_d, "defaults")
            with open(path, "w") as text_file:
                text_file.write(get_cfg_test_file_defaults())

            path = os.path.join(self._config_d, "fbsource.repo")
            with open(path, "w") as text_file:
                text_file.write(get_cfg_test_file_fbsource_repo())

            path = os.path.join(self._config_d, "hooks")
            with open(path, "w") as text_file:
                text_file.write(get_cfg_test_file_hooks())

            path = os.path.join(self._home_dir, ".edenrc")
            with open(path, "w") as text_file:
                text_file.write(get_cfg_test_file_user_rc())

        def copy_toml_config_files(self):
            path = os.path.join(self._config_d, "_use_toml_configs_")
            with open(path, "w") as text_file:
                text_file.write("")

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

        def assert_core_config(self, cfg):
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

        def assert_git_repo_config(self, cfg):
            cc = cfg.find_config_for_alias("git")
            self.assertEqual(cc.path, f"/home/{self._user}/src/git/.git")
            self.assertEqual(cc.scm_type, "git")
            self.assertEqual(cc.hooks_path, f"/home/{self._user}/my-git-hook")
            self.assertEqual(cc.bind_mounts, {})
            self.assertEqual(cc.default_revision, "master")

        def assert_fbsource_repo_config(self, cfg):
            cc = cfg.find_config_for_alias("fbsource")
            self.assertEqual(cc.path, f"/data/users/{self._user}/fbsource-override")
            self.assertEqual(cc.scm_type, "hg")
            self.assertEqual(
                cc.bind_mounts,
                {"fbcode-buck-out": "fbcode/buck-out-override", "buck-out": "buck-out"},
            )
            self.assertEqual(cc.default_revision, "master")

        def test_load_config(self):
            self.copy_config_files()
            cfg = config.Config(
                self._config_dir,
                self._etc_eden_dir,
                self._home_dir,
                self._interpolate_dict,
            )
            # Check if file _toml_config_exists_  is present. It should be
            # consistent with cfg._toml_config_exists
            toml_cfg_exists = cfg._toml_config_exists()
            self.assertEqual(toml_cfg_exists, cfg._use_toml_cfg)

            # Check the various config sections
            self.assert_core_config(cfg)
            exp_repos = ["fbsource", "git"]
            self.assertEqual(cfg.get_repository_list(), exp_repos)
            self.assert_fbsource_repo_config(cfg)
            self.assert_git_repo_config(cfg)

            # Check if test is for toml or cfg by cfg._user_toml_cfg
            toml_suffix = ".toml" if cfg._use_toml_cfg else ""
            exp_rc_files = [
                os.path.join(self._config_d, "defaults" + toml_suffix),
                os.path.join(self._config_d, "fbsource.repo" + toml_suffix),
                os.path.join(self._config_d, "hooks" + toml_suffix),
                os.path.join(self._home_dir, ".edenrc"),
            ]
            self.assertEqual(cfg.get_rc_files(), exp_rc_files)

        def test_no_dot_edenrc(self):
            self.copy_config_files()

            os.remove(os.path.join(self._home_dir, ".edenrc"))
            cfg = config.Config(
                self._config_dir,
                self._etc_eden_dir,
                self._home_dir,
                self._interpolate_dict,
            )
            cfg._loadConfig()

            exp_repos = ["fbsource"]
            self.assertEqual(cfg.get_repository_list(), exp_repos)

            self.assertEqual(
                cfg.get_config_value("rage.reporter"),
                'arc paste --title "eden rage from $(hostname)" --conduit-uri=https://phabricator.intern.facebook.com/api/',
            )
            self.assertEqual(
                cfg.get_config_value("core.ignoreFile"),
                f"/home/{self._user}/.gitignore",
            )
            self.assertEqual(
                cfg.get_config_value("core.systemIgnoreFile"), "/etc/eden/gitignore"
            )
            cc = cfg.find_config_for_alias("fbsource")
            self.assertEqual(cc.path, f"/data/users/{self._user}/fbsource")
            self.assertEqual(cc.scm_type, "hg")
            self.assertEqual(
                cc.bind_mounts,
                {"fbcode-buck-out": "fbcode/buck-out", "buck-out": "buck-out"},
            )
            self.assertEqual(cc.default_revision, "master")

        def test_add_existing_repo(self):
            self.copy_config_files()

            cfg = config.Config(
                self._config_dir,
                self._etc_eden_dir,
                self._home_dir,
                self._interpolate_dict,
            )
            with self.assertRaisesRegex(
                config.UsageError,
                "repository fbsource already exists. You will need to edit "
                "the ~/.edenrc config file by hand to make changes to the "
                "repository or remove it.",
            ):
                cfg.add_repository(
                    "fbsource", "hg", f"/data/users/{self._user}/fbsource"
                )

        def test_add_repo(self):
            self.copy_config_files()

            cfg = config.Config(
                self._config_dir,
                self._etc_eden_dir,
                self._home_dir,
                self._interpolate_dict,
            )
            cfg.add_repository("fbandroid", "hg", f"/data/users/{self._user}/fbandroid")

            # Lets reload our config
            cfg = config.Config(
                self._config_dir,
                self._etc_eden_dir,
                self._home_dir,
                self._interpolate_dict,
            )
            # Check the various config sections
            self.assert_core_config(cfg)
            exp_repos = ["fbandroid", "fbsource", "git"]
            self.assertEqual(cfg.get_repository_list(), exp_repos)
            self.assert_fbsource_repo_config(cfg)
            self.assert_git_repo_config(cfg)

            # Check the newly added repo
            cc = cfg.find_config_for_alias("fbandroid")
            self.assertEqual(cc.path, f"/data/users/{self._user}/fbandroid")
            self.assertEqual(cc.scm_type, "hg")
            self.assertEqual(cc.hooks_path, f"{self._etc_eden_dir}/hooks")
            self.assertEqual(cc.bind_mounts, {})
            self.assertEqual(cc.default_revision, "master")


class ConfigTest(TopConfigTestBase.ConfigTestBase):
    def copy_config_files(self):
        self.copy_cfg_config_files()


class TomlConfigTest(TopConfigTestBase.ConfigTestBase):
    def copy_config_files(self):
        self.copy_cfg_config_files()
        self.copy_toml_config_files()

    def test_config_string(self):
        # Stringification is used for displaying the results. Let's test we get
        # the same results with both
        self.copy_cfg_config_files()
        cfg = config.Config(
            self._config_dir, self._etc_eden_dir, self._home_dir, self._interpolate_dict
        )
        cfg_str = get_config_as_string(cfg._loadConfig())

        self.copy_toml_config_files()
        toml_cfg = config.Config(
            self._config_dir, self._etc_eden_dir, self._home_dir, self._interpolate_dict
        )
        toml_cfg_str = get_config_as_string(toml_cfg._loadConfig())
        self.assertEqual(toml_cfg_str, cfg_str)

    def test_toml_convert(self):
        # This test converts our config files to toml files and checks the
        # configuration is the same.  In this way, we are validating the
        # configutil and toml library use for creating and updating toml files.
        self.copy_cfg_config_files()
        paths = [
            os.path.join(self._home_dir, ".edenrc"),
            os.path.join(self._config_d, "defaults"),
            os.path.join(self._config_d, "hooks"),
            os.path.join(self._config_d, "fbsource.repo"),
        ]
        for path in paths:
            # Load config files configuration
            cfg = ForceFileMockConfig(
                self._config_dir,
                self._etc_eden_dir,
                self._home_dir,
                [path],
                {},  # No interpolation
                use_toml_cfg=False,
            )
            cfg_str = get_config_as_string(cfg._loadConfig())

            # Convert to toml and save as toml file
            toml_config = configutil.config_to_raw_dict(cfg._loadConfig())
            toml_data = toml.dumps(toml_config)
            toml_path = path + ".toml"
            with open(toml_path, "w") as text_file:
                text_file.write(toml_data)

            # Load the newly created toml files configuration
            toml_cfg = ForceFileMockConfig(
                self._config_dir,
                self._etc_eden_dir,
                self._home_dir,
                [toml_path],
                {},  # No interpolation
                use_toml_cfg=True,
            )
            toml_cfg_str = get_config_as_string(toml_cfg._loadConfig())

            # Check that the strings are equivalent
            self.assertEqual(cfg_str, toml_cfg_str)

    def test_toml_enable(self):
        cfg = config.Config(
            self._config_dir, self._etc_eden_dir, self._home_dir, self._interpolate_dict
        )
        self.assertFalse(cfg._toml_config_exists())

        path = os.path.join(self._config_d, "_use_toml_configs_")
        with open(path, "w") as text_file:
            text_file.write("")

        self.assertTrue(cfg._toml_config_exists())

    def test_toml_error(self):
        self.copy_toml_config_files()

        path = os.path.join(self._home_dir, ".edenrc")
        with open(path, "w") as text_file:
            text_file.write(get_toml_test_file_invalid())

        cfg = config.Config(
            self._config_dir, self._etc_eden_dir, self._home_dir, self._interpolate_dict
        )
        self.assertEqual(cfg._use_toml_cfg, True)
        with self.assertRaises(toml.decoder.TomlDecodeError):
            cfg._loadConfig()
