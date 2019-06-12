#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from pathlib import Path

from facebook.eden.eden_config.ttypes import (
    ConfigReloadBehavior,
    ConfigSource,
    EdenConfigData,
)
from facebook.eden.ttypes import GetConfigParams

from .lib import testcase


class ConfigTest(testcase.EdenTestCase):
    def assert_config(
        self, config: EdenConfigData, name: str, value: str, source: ConfigSource
    ) -> None:
        actual_value = config.values.get(name)
        self.assertIsNotNone(actual_value)
        assert actual_value is not None  # just to make the type checkers happy
        self.assertEqual(value, actual_value.parsedValue)
        self.assertEqual(source, actual_value.source)

    def test_get_config(self) -> None:
        self.maxDiff = None

        with self.get_thrift_client() as client:
            # Check the initial config values
            config = client.getConfig(GetConfigParams())

            # The edenDirectory property is currently always recorded as being set from
            # the command line, regardless of how it was actually determined.
            # (This is to ensure it cannot later be overwitten by a config file change
            # once edenfs has started.)
            self.assert_config(
                config, "core:edenDirectory", self.eden_dir, ConfigSource.CommandLine
            )
            self.assert_config(
                config,
                "core:ignoreFile",
                str(Path(self.home_dir) / ".edenignore"),
                ConfigSource.Default,
            )
            self.assert_config(
                config,
                "core:systemIgnoreFile",
                str(Path(self.etc_eden_dir) / "ignore"),
                ConfigSource.Default,
            )

            # Update the config on disk
            user_config_path = Path(self.home_dir) / ".edenrc"
            new_ignore_path = Path(self.home_dir) / ".gitignore"
            user_config_path.write_text(
                f"""\
[core]
ignoreFile = "{new_ignore_path}"
"""
            )

            # Get the config, asking just for the cached config values without
            # attempting to reload them from disk.
            config = client.getConfig(
                GetConfigParams(reload=ConfigReloadBehavior.NoReload)
            )
            # The ignore path should be unchanged
            self.assert_config(
                config,
                "core:ignoreFile",
                str(Path(self.home_dir) / ".edenignore"),
                ConfigSource.Default,
            )

            # Now get the config, asking for a reload
            # attempting to reload them from disk.
            config = client.getConfig(
                GetConfigParams(reload=ConfigReloadBehavior.ForceReload)
            )
            # The ignore path should be updated
            self.assert_config(
                config, "core:ignoreFile", str(new_ignore_path), ConfigSource.UserConfig
            )
