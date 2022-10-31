#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import configparser
import io
import os
import sys
import unittest
from collections import namedtuple
from pathlib import Path
from typing import Dict
from unittest.mock import MagicMock, patch

import toml
import toml.decoder
from eden.fs.cli.config import EdenInstance
from eden.fs.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.test_support.temporary_directory import TemporaryDirectoryMixin
from eden.test_support.testcase import EdenTestCaseBase

from .. import config as config_mod, configutil, util
from ..configinterpolator import EdenConfigInterpolator
from ..configutil import EdenConfigParser, UnexpectedType


def get_toml_test_file_invalid() -> str:
    cfg_file = """
[core thisIsNotAllowed]
"""
    return cfg_file


def get_toml_test_file_defaults() -> str:
    cfg_file = """
[core]
systemIgnoreFile = "/etc/eden/gitignore"
ignoreFile = "/home/${USER}/.gitignore"

[clone]
default-revision = "master"

[rage]
reporter = 'pastry --title "eden rage from $(hostname)"'
"""
    return cfg_file


def get_toml_test_file_user_rc() -> str:
    cfg_file = """
[core]
ignoreFile = "/home/${USER}/.gitignore-override"
edenDirectory = "/home/${USER}/.eden"

["telemetry"]
scribe-cat = "/usr/local/bin/scribe_cat"
"""
    return cfg_file


def get_toml_test_file_system_rc() -> str:
    cfg_file = """
["telemetry"]
scribe-cat = "/bad/path/to/scribe_cat"
"""
    return cfg_file


class TomlConfigTest(EdenTestCaseBase):
    def setUp(self) -> None:
        super().setUp()
        self._user = "bob"
        self._state_dir = self.tmp_dir / ".eden"
        self._etc_eden_dir = self.tmp_dir / "etc/eden"
        self._config_d = self.tmp_dir / "etc/eden/config.d"
        self._home_dir = self.tmp_dir / "home" / self._user
        self._interpolate_dict = {
            "USER": self._user,
            "USER_ID": "42",
            "HOME": str(self._home_dir),
        }

        self._state_dir.mkdir()
        self._config_d.mkdir(exist_ok=True, parents=True)
        self._home_dir.mkdir(exist_ok=True, parents=True)

    def copy_config_files(self) -> None:
        path = self._config_d / "defaults.toml"
        path.write_text(get_toml_test_file_defaults())

        path = self._home_dir / ".edenrc"
        path.write_text(get_toml_test_file_user_rc())

        path = self._etc_eden_dir / "edenfs.rc"
        path.write_text(get_toml_test_file_system_rc())

    def assert_core_config(self, cfg: EdenInstance) -> None:
        self.assertEqual(
            cfg.get_config_value("rage.reporter", default=""),
            'pastry --title "eden rage from $(hostname)"',
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

    def assert_config_precedence(self, cfg: EdenInstance) -> None:
        self.assertEqual(
            cfg.get_config_value("telemetry.scribe-cat", default=""),
            "/usr/local/bin/scribe_cat",
        )

    def test_load_config(self) -> None:
        self.copy_config_files()
        cfg = self.get_config()

        # Check the various config sections
        self.assert_core_config(cfg)
        self.assert_config_precedence(cfg)

        # Check if test is for toml or cfg by cfg._user_toml_cfg
        exp_rc_files = [
            self._config_d / "defaults.toml",
            self._etc_eden_dir / "edenfs.rc",
            self._home_dir / ".edenrc",
        ]
        self.assertEqual(cfg.get_rc_files(), exp_rc_files)

    def test_no_dot_edenrc(self) -> None:
        self.copy_config_files()

        (self._home_dir / ".edenrc").unlink()
        cfg = self.get_config()
        cfg._loadConfig()

        self.assertEqual(
            cfg.get_config_value("rage.reporter", default=""),
            'pastry --title "eden rage from $(hostname)"',
        )
        self.assertEqual(
            cfg.get_config_value("core.ignoreFile", default=""),
            f"/home/{self._user}/.gitignore",
        )
        self.assertEqual(
            cfg.get_config_value("core.systemIgnoreFile", default=""),
            "/etc/eden/gitignore",
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

    def test_user_id_variable_is_set_to_process_uid(self) -> None:
        config = self.get_config_without_stub_variables()
        self.write_user_config(
            """
[testsection]
testoption = "My user ID is ${USER_ID}."
"""
        )

        uid = os.getuid() if sys.platform != "win32" else 0
        self.assertEqual(
            config.get_config_value("testsection.testoption", default=""),
            f"My user ID is {uid}.",
        )

    def test_printed_config_is_valid_toml(self) -> None:
        self.write_user_config(
            """
[clone]
default-revision = "master"
"""
        )

        printed_config = io.BytesIO()
        self.get_config().print_full_config(printed_config)
        parsed_config = printed_config.getvalue().decode("utf-8")
        parsed_toml = toml.loads(parsed_config)

        self.assertIn("clone", parsed_toml)
        self.assertEqual(parsed_toml["clone"].get("default-revision"), "master")

    def test_printed_config_expands_variables(self) -> None:
        self.write_user_config(
            """
["repository fbsource"]
type = "hg"
path = "/data/users/${USER}/fbsource"
"""
        )

        printed_config = io.BytesIO()
        self.get_config().print_full_config(printed_config)

        self.assertIn(b"/data/users/bob/fbsource", printed_config.getvalue())

    def test_printed_config_writes_booleans_as_booleans(self) -> None:
        self.write_user_config(
            """
[experimental]
use-edenapi = true
"""
        )

        printed_config = io.BytesIO()
        self.get_config().print_full_config(printed_config)
        parsed_config = printed_config.getvalue().decode("utf-8")

        self.assertRegex(parsed_config, r"use-edenapi\s*=\s*true")

    def get_config(self) -> EdenInstance:
        return EdenInstance(
            self._state_dir, self._etc_eden_dir, self._home_dir, self._interpolate_dict
        )

    def get_config_without_stub_variables(self) -> EdenInstance:
        return EdenInstance(
            self._state_dir, self._etc_eden_dir, self._home_dir, interpolate_dict=None
        )

    def write_user_config(self, content: str) -> None:
        (self._home_dir / ".edenrc").write_text(content)


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
        self,
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
        section: str = expectation.exception.section
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


class EdenInstanceConstructionTest(unittest.TestCase):
    def test_full_cmd_line(self) -> None:
        cmdline = [
            b"/usr/local/libexec/eden/edenfs",
            b"--edenfs",
            b"--edenDir",
            b"/data/users/testuser/.eden",
            b"--etcEdenDir",
            b"/etc/eden",
            b"--configPath",
            b"/home/testuser/.edenrc",
            b"--edenfsctlPath",
            b"/usr/local/bin/edenfsctl",
            b"--takeover",
            b"",
        ]
        instance = config_mod.eden_instance_from_cmdline(cmdline)
        self.assertEqual(instance.state_dir, Path("/data/users/testuser/.eden"))
        self.assertEqual(instance.etc_eden_dir, Path("/etc/eden"))
        self.assertEqual(instance.home_dir, Path("/home/testuser/"))

    def test_sparse_cmd_line(self) -> None:
        cmdline = [
            b"/usr/local/libexec/eden/edenfs",
            b"--edenfs",
            b"--etcEdenDir",
            b"/etc/eden",
            b"--configPath",
            b"/home/testuser/.edenrc",
            b"--edenfsctlPath",
            b"/usr/local/bin/edenfsctl",
            b"--takeover",
            b"",
        ]
        instance = config_mod.eden_instance_from_cmdline(cmdline)

        self.assertEqual(
            instance.state_dir, Path("/home/testuser/local/.eden").resolve()
        )
        self.assertEqual(instance.etc_eden_dir, Path("/etc/eden"))
        self.assertEqual(instance.home_dir, Path("/home/testuser/"))

    def test_malformed_cmd_line(self) -> None:
        cmdline = [
            b"/usr/local/libexec/eden/edenfs",
            b"--configPath",
            b"/home/testuser/.edenrc",
        ]
        instance = config_mod.eden_instance_from_cmdline(cmdline)

        self.assertEqual(
            instance.state_dir, Path("/home/testuser/local/.eden").resolve()
        )
        self.assertEqual(instance.etc_eden_dir, Path("/etc/eden"))
        self.assertEqual(instance.home_dir, Path("/home/testuser/"))


class NFSMigrationTest(EdenTestCaseBase):
    def setUp(self) -> None:
        super().setUp()
        self._user = "bob"
        self._state_dir = self.tmp_dir / ".eden"
        self._etc_eden_dir = self.tmp_dir / "etc/eden"
        self._config_d = self.tmp_dir / "etc/eden/config.d"
        self._home_dir = self.tmp_dir / "home" / self._user
        self._interpolate_dict = {
            "USER": self._user,
            "USER_ID": "42",
            "HOME": str(self._home_dir),
        }

        self._state_dir.mkdir()
        self._config_d.mkdir(exist_ok=True, parents=True)
        self._home_dir.mkdir(exist_ok=True, parents=True)

    def setup_config_files(self, mounts: Dict[str, str]) -> None:
        config_json_list = ",\n".join(
            [f'"{self.tmp_dir}/{mount}" : "{mount}"' for mount in mounts]
        )
        config_json = f"""{{
{config_json_list}
}}"""
        (self._state_dir / "config.json").write_text(config_json)

        (self._state_dir / "clients").mkdir()
        for mount, initial_mount_protocol in mounts.items():
            (self._state_dir / "clients" / mount).mkdir()
            (self._state_dir / "clients" / mount / "config.toml").write_text(
                f"""
[repository]
path = "{self.tmp_dir}/.eden-backing-repos/test"
type = "hg"
protocol = "{initial_mount_protocol}"
"""
            )

    @patch("eden.fs.cli.util.is_sandcastle", return_value=False)
    @patch("os.uname")
    def check_should_migrate(
        self,
        platform: str,
        os_version: str,
        config: Dict[str, str],
        mock_uname: MagicMock,
        mock_is_sandcastle: MagicMock,
    ) -> bool:
        FakeUname = namedtuple("FakeUname", ["release"])

        with patch("sys.platform", platform):
            mock_uname.return_value = FakeUname(release=os_version)
            instance = FakeEdenInstance(
                str(self.temp_mgr.make_temp_dir()), config=config
            )
            return config_mod.should_migrate_mount_protocol_to_nfs(instance)

    def test_should_migrate_monterey_no_config(self) -> None:
        self.assertFalse(self.check_should_migrate("darwin", "21.0.0", {}))

    def test_should_migrate_monterey_with_config(self) -> None:
        self.assertFalse(
            self.check_should_migrate(
                "darwin",
                "21.0.0",
                {
                    "core.migrate_existing_to_nfs": "true",
                },
            )
        )

    def test_should_migrate_monterey_with_config_all_versions(self) -> None:
        self.assertTrue(
            self.check_should_migrate(
                "darwin",
                "21.0.0",
                {
                    "core.migrate_existing_to_nfs_all_macos": "true",
                },
            )
        )
        self.assertTrue(
            self.check_should_migrate(
                "darwin",
                "21.0.0",
                {
                    "core.migrate_existing_to_nfs": "true",
                    "core.migrate_existing_to_nfs_all_macos": "true",
                },
            )
        )

        # Even if the Ventura-specific config is false, the _all_versions config
        # can still trigger the migration.
        self.assertTrue(
            self.check_should_migrate(
                "darwin",
                "21.0.0",
                {
                    "core.migrate_existing_to_nfs": "false",
                    "core.migrate_existing_to_nfs_all_macos": "true",
                },
            )
        )

    def test_should_migrate_ventura_no_config(self) -> None:
        self.assertFalse(self.check_should_migrate("darwin", "22.0.0", {}))

    def test_should_migrate_ventura_with_config(self) -> None:
        self.assertTrue(
            self.check_should_migrate(
                "darwin",
                "22.0.0",
                {
                    "core.migrate_existing_to_nfs": "true",
                },
            )
        )

        # Even if the _all_versions config is false, the Ventura-specific config
        # can still trigger the migration.
        self.assertTrue(
            self.check_should_migrate(
                "darwin",
                "22.0.0",
                {
                    "core.migrate_existing_to_nfs": "true",
                    "core.migrate_existing_to_nfs_all_macos": "false",
                },
            )
        )

    def test_should_migrate_ventura_with_config_all_versions(self) -> None:
        self.assertTrue(
            self.check_should_migrate(
                "darwin",
                "22.0.0",
                {
                    "core.migrate_existing_to_nfs_all_macos": "true",
                },
            )
        )
        self.assertTrue(
            self.check_should_migrate(
                "darwin",
                "22.0.0",
                {
                    "core.migrate_existing_to_nfs": "true",
                    "core.migrate_existing_to_nfs_all_macos": "true",
                },
            )
        )

        # Even if the Ventura-specific config is false, the _all_versions config
        # can still trigger the migration.
        self.assertTrue(
            self.check_should_migrate(
                "darwin",
                "22.0.0",
                {
                    "core.migrate_existing_to_nfs": "false",
                    "core.migrate_existing_to_nfs_all_macos": "true",
                },
            )
        )

    def test_should_migrate_non_macos(self) -> None:
        self.assertFalse(
            self.check_should_migrate(
                "linux",
                "22.0.0",
                {
                    "core.migrate_existing_to_nfs": "true",
                    "core.migrate_existing_to_nfs_all_macos": "true",
                },
            )
        )

    def check_nfs_migrations_needing_full_restart(
        self, platform: str, config: Dict[str, str], mounts: Dict[str, str]
    ) -> int:
        instance = FakeEdenInstance(str(self.temp_mgr.make_temp_dir()), config=config)
        for mount, protocol in mounts.items():
            instance.create_test_mount(mount, mount_protocol=protocol)

        with patch("sys.platform", platform):
            return config_mod.count_nfs_migrations_needing_full_restart(instance)

    def test_nfs_migrations_needing_full_restart_no_config_no_mounts(self) -> None:
        self.assertEqual(
            0, self.check_nfs_migrations_needing_full_restart("darwin", {}, {})
        )

    def test_nfs_migrations_needing_full_restart_no_config_no_fuse_mounts(self) -> None:
        self.assertEqual(
            0,
            self.check_nfs_migrations_needing_full_restart(
                "darwin", {}, {"foo": "nfs"}
            ),
        )

    def test_nfs_migrations_needing_full_restart_yes_config_no_fuse_mounts(
        self,
    ) -> None:
        self.assertEqual(
            0,
            self.check_nfs_migrations_needing_full_restart(
                "darwin",
                {"core.migrate_existing_to_nfs_all_macos": "true"},
                {"foo": "nfs"},
            ),
        )

    def test_nfs_migrations_needing_full_restart_no_config_yes_fuse_mounts(
        self,
    ) -> None:
        self.assertEqual(
            0,
            self.check_nfs_migrations_needing_full_restart(
                "darwin", {}, {"foo": "fuse"}
            ),
        )

    def test_nfs_migrations_needing_full_restart_yes_config_yes_fuse_mounts(
        self,
    ) -> None:
        self.assertEqual(
            2,
            self.check_nfs_migrations_needing_full_restart(
                "darwin",
                {"core.migrate_existing_to_nfs_all_macos": "true"},
                {"foo": "fuse", "bar": "nfs", "baz": "fuse"},
            ),
        )

    def test_nfs_migrations_needing_full_restart_ventura_config(self) -> None:
        self.assertEqual(
            0,
            self.check_nfs_migrations_needing_full_restart(
                "darwin",
                {"core.migrate_existing_to_nfs": "true"},
                {"foo": "fuse"},
            ),
        )

    def test_nfs_migrations_needing_full_restart_linux(self) -> None:
        self.assertEqual(
            0,
            self.check_nfs_migrations_needing_full_restart(
                "linux",
                {"core.migrate_existing_to_nfs_all_macos": "true"},
                {"foo": "fuse"},
            ),
        )

    def test_nfs_migrations_needing_full_restart_windows(self) -> None:
        self.assertEqual(
            0,
            self.check_nfs_migrations_needing_full_restart(
                "win32",
                {"core.migrate_existing_to_nfs_all_macos": "true"},
                {"foo": "prjfs"},
            ),
        )

    def check_migrate_nfs(self, mounts: Dict[str, str]) -> None:
        self.setup_config_files(mounts)

        cmdline = [
            b"/usr/local/libexec/eden/edenfs",
            b"--edenfs",
            b"--edenDir",
            str(self._state_dir).encode("utf-8"),
            b"--etcEdenDir",
            str(self._config_d).encode("utf-8"),
            b"--configPath",
            str(self._home_dir).encode("utf-8"),
            b"--edenfsctlPath",
            b"/usr/local/bin/edenfsctl",
            b"--takeover",
            b"",
        ]
        instance = config_mod.eden_instance_from_cmdline(cmdline)

        for mount, initial_mount_protocol in mounts.items():
            checkoutConfig = config_mod.EdenCheckout(
                instance, self.tmp_dir / mount, self._state_dir / "clients" / mount
            ).get_config()

            self.assertEqual(checkoutConfig.mount_protocol, initial_mount_protocol)

        config_mod._do_nfs_migration(instance, lambda protocol: protocol)

        for mount in mounts:
            checkoutConfig = config_mod.EdenCheckout(
                instance, self.tmp_dir / mount, self._state_dir / "clients" / mount
            ).get_config()

            self.assertEqual(checkoutConfig.mount_protocol, "nfs")

    def test_none(self) -> None:
        mounts = {}
        self.check_migrate_nfs(mounts)

    def test_simple(self) -> None:
        mounts = {"test": "fuse"}
        self.check_migrate_nfs(mounts)

    def test_multiple(self) -> None:
        mounts = {"test1": "fuse", "test2": "fuse"}
        self.check_migrate_nfs(mounts)

    def test_already_nfs(self) -> None:
        mounts = {"test": "nfs"}
        self.check_migrate_nfs(mounts)

    def test_multiple_nfs_fuse(self) -> None:
        mounts = {"test1": "nfs", "test2": "fuse", "test3": "fuse", "test4": "nfs"}
        self.check_migrate_nfs(mounts)
