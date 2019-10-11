#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import configparser
import io
import os
import unittest
from pathlib import Path

import toml
import toml.decoder
from eden.test_support.environment_variable import EnvironmentVariableMixin
from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from .. import config as config_mod, configutil, util
from ..config import EdenInstance
from ..configinterpolator import EdenConfigInterpolator
from ..configutil import EdenConfigParser, UnexpectedType


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
        self._interpolate_dict = {
            "USER": self._user,
            "USER_ID": "42",
            "HOME": self._home_dir,
        }

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

        path = os.path.join(self._home_dir, ".edenrc")
        with open(path, "w") as text_file:
            text_file.write(get_toml_test_file_user_rc())

    def assert_core_config(self, cfg: EdenInstance) -> None:
        self.assertEqual(
            cfg.get_config_value("rage.reporter", default=""),
            'arc paste --title "eden rage from $(hostname)" --conduit-uri=https://phabricator.intern.facebook.com/api/',
        )
        self.assertEqual(
            cfg.get_config_value("core.ignoreFile", default=""),
            f"/home/{self._user}/.gitignore-override",
        )
        self.assertEqual(
            cfg.get_config_value("core.systemIgnoreFile", default=""),
            "/etc/eden/gitignore",
        )
        self.assertEqual(
            cfg.get_config_value("core.edenDirectory", default=""),
            f"/home/{self._user}/.eden",
        )

    def assert_git_repo_config(self, cfg: EdenInstance) -> None:
        cc = cfg.find_config_for_alias("git")
        assert cc is not None
        self.assertEqual(cc.backing_repo, Path(f"/home/{self._user}/src/git/.git"))
        self.assertEqual(cc.scm_type, "git")
        self.assertEqual(cc.bind_mounts, {})
        self.assertEqual(cc.default_revision, "master")

    def assert_fbsource_repo_config(self, cfg: EdenInstance) -> None:
        cc = cfg.find_config_for_alias("fbsource")
        assert cc is not None
        self.assertEqual(
            cc.backing_repo, Path(f"/data/users/{self._user}/fbsource-override")
        )
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
            Path(self._config_d) / "defaults.toml",
            Path(self._config_d) / "fbsource.repo.toml",
            Path(self._home_dir) / ".edenrc",
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
            cfg.get_config_value("rage.reporter", default=""),
            'arc paste --title "eden rage from $(hostname)" --conduit-uri=https://phabricator.intern.facebook.com/api/',
        )
        self.assertEqual(
            cfg.get_config_value("core.ignoreFile", default=""),
            f"/home/{self._user}/.gitignore",
        )
        self.assertEqual(
            cfg.get_config_value("core.systemIgnoreFile", default=""),
            "/etc/eden/gitignore",
        )
        cc = cfg.find_config_for_alias("fbsource")
        assert cc is not None
        self.assertEqual(cc.backing_repo, Path(f"/data/users/{self._user}/fbsource"))
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
        self.assertEqual(cc.backing_repo, Path(f"/data/users/{self._user}/fbandroid"))
        self.assertEqual(cc.scm_type, "hg")
        self.assertEqual(cc.bind_mounts, {})
        self.assertEqual(cc.default_revision, "master")

    def test_missing_type_option_in_repository_is_an_error(self) -> None:
        self.write_user_config(
            """
["repository myrepo"]
path = "/tmp/myrepo"
"""
        )
        with self.assertRaises(Exception) as expectation:
            cfg = self.get_config()
            cfg.find_config_for_alias("myrepo")
        self.assertEqual(
            str(expectation.exception), 'repository "myrepo" missing key "type".'
        )

    def test_invalid_type_option_in_repository_is_an_error(self) -> None:
        self.write_user_config(
            """
["repository myrepo"]
type = "invalidrepotype"
path = "/tmp/myrepo"
"""
        )
        with self.assertRaises(Exception) as expectation:
            cfg = self.get_config()
            cfg.find_config_for_alias("myrepo")
        self.assertEqual(
            str(expectation.exception), 'repository "myrepo" has unsupported type.'
        )

    def test_empty_type_option_in_repository_is_an_error(self) -> None:
        self.write_user_config(
            """
["repository myrepo"]
type = ""
path = "/tmp/myrepo"
"""
        )
        with self.assertRaises(Exception) as expectation:
            cfg = self.get_config()
            cfg.find_config_for_alias("myrepo")
        self.assertEqual(
            str(expectation.exception), 'repository "myrepo" missing key "type".'
        )

    def test_missing_path_option_in_repository_is_an_error(self) -> None:
        self.write_user_config(
            """
["repository myrepo"]
type = "hg"
"""
        )
        with self.assertRaises(Exception) as expectation:
            cfg = self.get_config()
            cfg.find_config_for_alias("myrepo")
        self.assertEqual(
            str(expectation.exception), 'repository "myrepo" missing key "path".'
        )

    def test_empty_path_option_in_repository_is_an_error(self) -> None:
        self.write_user_config(
            """
["repository myrepo"]
type = "hg"
path = ""
"""
        )
        with self.assertRaises(Exception) as expectation:
            cfg = self.get_config()
            cfg.find_config_for_alias("myrepo")
        self.assertEqual(
            str(expectation.exception), 'repository "myrepo" missing key "path".'
        )

    def test_toml_error(self) -> None:
        self.copy_config_files()

        self.write_user_config(get_toml_test_file_invalid())

        cfg = self.get_config()
        with self.assertRaises(toml.decoder.TomlDecodeError):
            cfg._loadConfig()

    def test_get_config_value_returns_default_if_section_is_missing(self) -> None:
        self.assertEqual(
            self.get_config().get_config_value(
                "missing_section.test_option", default="test default"
            ),
            "test default",
        )

    def test_get_config_value_returns_default_if_option_is_missing(self) -> None:
        self.write_user_config(
            """[test_section]
other_option = "test value"
"""
        )
        self.assertEqual(
            self.get_config().get_config_value(
                "test_section.missing_option", default="test default"
            ),
            "test default",
        )

    def test_get_config_value_returns_value_for_string_option(self) -> None:
        self.write_user_config(
            """[test_section]
test_option = "test value"
"""
        )
        self.assertEqual(
            self.get_config().get_config_value(
                "test_section.test_option", default="test default"
            ),
            "test value",
        )

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

    def test_user_id_variable_is_set_to_process_uid(self) -> None:
        config = self.get_config_without_stub_variables()
        self.write_user_config(
            """
[testsection]
testoption = "My user ID is ${USER_ID}."
"""
        )
        self.assertEqual(
            config.get_config_value("testsection.testoption", default=""),
            f"My user ID is {os.getuid()}.",
        )

    def test_default_fallback_systemd_xdg_runtime_dir_is_run_user_uid(self) -> None:
        self.assertEqual(
            self.get_config().get_fallback_systemd_xdg_runtime_dir(), "/run/user/42"
        )

    def test_configured_fallback_systemd_xdg_runtime_dir_expands_user_and_user_id(
        self
    ) -> None:
        self.write_user_config(
            """
[service]
fallback_systemd_xdg_runtime_dir = "/var/run/${USER}/${USER_ID}"
"""
        )
        self.assertEqual(
            self.get_config().get_fallback_systemd_xdg_runtime_dir(), "/var/run/bob/42"
        )

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

    def test_printed_config_writes_booleans_as_booleans(self) -> None:
        self.write_user_config(
            """
[service]
experimental_systemd = true
"""
        )

        printed_config = io.StringIO()
        self.get_config().print_full_config(file=printed_config)

        self.assertRegex(printed_config.getvalue(), r"experimental_systemd\s*=\s*true")

    def get_config(self) -> EdenInstance:
        return EdenInstance(
            self._state_dir, self._etc_eden_dir, self._home_dir, self._interpolate_dict
        )

    def get_config_without_stub_variables(self) -> EdenInstance:
        return EdenInstance(
            self._state_dir, self._etc_eden_dir, self._home_dir, interpolate_dict=None
        )

    def write_user_config(self, content: str) -> None:
        path = os.path.join(self._home_dir, ".edenrc")
        with open(path, "w") as text_file:
            text_file.write(content)


class EdenConfigParserTest(unittest.TestCase):
    unsupported_value = {"dict of string to string": ""}

    def test_loading_config_with_unsupported_type_is_not_an_error(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"test_option": self.unsupported_value}})

    def test_querying_bool_returns_bool(self) -> None:
        for value in [True, False]:
            with self.subTest(value=value):
                parser = EdenConfigParser()
                parser.read_dict({"test_section": {"test_option": value}})
                self.assertEqual(
                    parser.get_bool("test_section", "test_option", default=True), value
                )
                self.assertEqual(
                    parser.get_bool("test_section", "test_option", default=False), value
                )

    def test_querying_bool_with_non_boolean_value_fails(self) -> None:
        for value in ["not a boolean", "", "true", "True", 0]:
            with self.subTest(value=value):
                parser = EdenConfigParser()
                parser.read_dict({"test_section": {"test_option": value}})
                with self.assertRaises(UnexpectedType) as expectation:
                    parser.get_bool("test_section", "test_option", default=False)
                self.assertEqual(expectation.exception.section, "test_section")
                self.assertEqual(expectation.exception.option, "test_option")
                self.assertEqual(expectation.exception.value, value)
                self.assertEqual(expectation.exception.expected_type, bool)

    def test_querying_bool_with_value_of_unsupported_type_fails(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"test_option": self.unsupported_value}})
        with self.assertRaises(UnexpectedType) as expectation:
            parser.get_bool("test_section", "test_option", default=False)
        self.assertEqual(expectation.exception.section, "test_section")
        self.assertEqual(expectation.exception.option, "test_option")
        self.assertEqual(expectation.exception.value, self.unsupported_value)
        self.assertEqual(expectation.exception.expected_type, bool)

    def test_querying_str_with_non_string_value_fails(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"test_option": True}})
        with self.assertRaises(UnexpectedType) as expectation:
            parser.get_str("test_section", "test_option", default="")
        self.assertEqual(expectation.exception.section, "test_section")
        self.assertEqual(expectation.exception.option, "test_option")
        self.assertEqual(expectation.exception.value, True)
        self.assertEqual(expectation.exception.expected_type, str)

    def test_querying_section_str_to_str_returns_mapping(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"a": "a value", "b": "b value"}})
        section = parser.get_section_str_to_str("test_section")
        self.assertCountEqual(section, {"a", "b"})
        self.assertEqual(section["a"], "a value")
        self.assertEqual(section["b"], "b value")

    def test_querying_section_str_to_any_fails_if_option_has_unsupported_type(
        self
    ) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"unsupported": self.unsupported_value}})
        with self.assertRaises(UnexpectedType) as expectation:
            parser.get_section_str_to_any("test_section")
        self.assertEqual(expectation.exception.section, "test_section")
        self.assertEqual(expectation.exception.option, "unsupported")
        self.assertEqual(expectation.exception.value, self.unsupported_value)
        self.assertIsNone(expectation.exception.expected_type)

    def test_querying_section_str_to_any_interpolates_options(self) -> None:
        parser = EdenConfigParser(
            interpolation=EdenConfigInterpolator({"USER": "alice"})
        )
        parser.read_dict({"test_section": {"test_option": "hello ${USER}"}})
        section = parser.get_section_str_to_any("test_section")
        self.assertEqual(section.get("test_option"), "hello alice")

    def test_querying_section_str_to_any_returns_any_supported_type(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict(
            {
                "test_section": {
                    "bool_option": True,
                    "string_array_option": ["hello", "world"],
                    "string_option": "hello",
                }
            }
        )
        section = parser.get_section_str_to_any("test_section")
        self.assertEqual(section["bool_option"], True)
        self.assertEqual(list(section["string_array_option"]), ["hello", "world"])
        self.assertEqual(section["string_option"], "hello")

    def test_querying_section_str_to_str_with_non_string_value_fails(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"a": False}})
        with self.assertRaises(UnexpectedType) as expectation:
            parser.get_section_str_to_str("test_section")
        self.assertEqual(expectation.exception.section, "test_section")
        self.assertEqual(expectation.exception.option, "a")
        self.assertEqual(expectation.exception.value, False)
        self.assertEqual(expectation.exception.expected_type, str)

    def test_querying_section_str_to_str_of_missing_section_fails(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"a": "a value"}})
        with self.assertRaises(configparser.NoSectionError) as expectation:
            parser.get_section_str_to_str("not_test_section")
        section: str = expectation.exception.section  # type: ignore
        self.assertEqual(section, "not_test_section")

    def test_querying_strs_with_empty_array_returns_empty_sequence(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"test_option": []}})
        self.assertEqual(
            list(
                parser.get_strs(
                    "test_section", "test_option", default=["default value"]
                )
            ),
            [],
        )

    def test_querying_strs_with_array_of_strings_returns_strs(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"test_option": ["first", "second", "3rd"]}})
        self.assertEqual(
            list(parser.get_strs("test_section", "test_option", default=[])),
            ["first", "second", "3rd"],
        )

    def test_querying_strs_with_array_of_non_strings_fails(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"test_option": [123]}})
        with self.assertRaises(UnexpectedType) as expectation:
            parser.get_strs("test_section", "test_option", default=[])
        self.assertEqual(expectation.exception.section, "test_section")
        self.assertEqual(expectation.exception.option, "test_option")
        self.assertEqual(expectation.exception.value, [123])
        self.assertEqual(expectation.exception.expected_type, configutil.Strs)

    def test_querying_missing_value_as_strs_returns_default(self) -> None:
        parser = EdenConfigParser()
        parser.read_dict({"test_section": {"bogus_option": []}})
        self.assertEqual(
            list(
                parser.get_strs(
                    "test_section", "missing_option", default=["default value"]
                )
            ),
            ["default value"],
        )

    def test_str_sequences_are_interpolated(self) -> None:
        parser = EdenConfigParser(
            interpolation=EdenConfigInterpolator({"USER": "alice"})
        )
        parser.read_dict(
            {
                "test_section": {
                    "test_option": ["sudo", "-u", "${USER}", "echo", "Hello, ${USER}!"]
                }
            }
        )
        self.assertEqual(
            list(parser.get_strs("test_section", "test_option", default=[])),
            ["sudo", "-u", "alice", "echo", "Hello, alice!"],
        )

    def test_unexpected_type_error_messages_are_helpful(self) -> None:
        self.assertEqual(
            'Expected boolean for service.experimental_systemd, but got string: "true"',
            str(
                UnexpectedType(
                    section="service",
                    option="experimental_systemd",
                    value="true",
                    expected_type=bool,
                )
            ),
        )

        self.assertEqual(
            "Expected string for repository myrepo.path, but got boolean: true",
            str(
                UnexpectedType(
                    section="repository myrepo",
                    option="path",
                    value=True,
                    expected_type=str,
                )
            ),
        )

        self.assertRegex(
            str(
                UnexpectedType(
                    section="section", option="option", value={}, expected_type=None
                )
            ),
            r"^Unexpected dict for section.option: \{\s*\}$",
        )

        self.assertEqual(
            "Expected array of strings for service.command, but got array: [ 123,]",
            str(
                UnexpectedType(
                    section="service",
                    option="command",
                    value=[123],
                    expected_type=configutil.Strs,
                )
            ),
        )
