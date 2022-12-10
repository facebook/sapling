#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import time
from pathlib import Path

from eden.thrift.legacy import EdenClient
from facebook.eden.eden_config.ttypes import (
    ConfigReloadBehavior,
    ConfigSourceType,
    EdenConfigData,
)
from facebook.eden.ttypes import GetConfigParams

from .lib import testcase


@testcase.eden_test
class ConfigTest(testcase.EdenTestCase):
    enable_logview: bool = False

    def assert_config(
        self,
        config: EdenConfigData,
        name: str,
        value: str,
        sourceType: ConfigSourceType,
    ) -> None:
        actual_value = config.values.get(name)
        self.assertIsNotNone(actual_value)
        assert actual_value is not None  # just to make the type checkers happy
        self.assertEqual(value, actual_value.parsedValue)
        self.assertEqual(sourceType, actual_value.sourceType)

    def test_get_config(self) -> None:
        self.maxDiff = None

        with self.get_thrift_client_legacy() as client:
            # Check the initial config values
            config = client.getConfig(GetConfigParams())

            # The edenDirectory property is currently always recorded as being set from
            # the command line, regardless of how it was actually determined.
            # (This is to ensure it cannot later be overwitten by a config file change
            # once edenfs has started.)
            self.assert_config(
                config,
                "core:edenDirectory",
                self.eden_dir,
                ConfigSourceType.CommandLine,
            )
            self.assert_config(
                config,
                "core:ignoreFile",
                str(Path(self.home_dir) / ".edenignore"),
                ConfigSourceType.Default,
            )
            self.assert_config(
                config,
                "core:systemIgnoreFile",
                str(Path(self.etc_eden_dir) / "ignore"),
                ConfigSourceType.Default,
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
                ConfigSourceType.Default,
            )

            # Now get the config, asking for a reload
            # attempting to reload them from disk.
            config = client.getConfig(
                GetConfigParams(reload=ConfigReloadBehavior.ForceReload)
            )
            # The ignore path should be updated
            self.assert_config(
                config,
                "core:ignoreFile",
                str(new_ignore_path),
                ConfigSourceType.UserConfig,
            )

    def test_periodic_reload(self) -> None:
        with self.get_thrift_client_legacy() as client:
            self._test_periodic_reload(client)

    def _test_periodic_reload(self, client: EdenClient) -> None:
        def write_user_config(reload_interval: str) -> None:
            config_text = f"""
[config]
reload-interval = "{reload_interval}"
"""
            self.eden.user_rc_path.write_text(config_text)

        def assert_current_interval(expected: str) -> None:
            no_reload_params = GetConfigParams(reload=ConfigReloadBehavior.NoReload)
            config = client.getConfig(no_reload_params)
            current_interval = config.values["config:reload-interval"].parsedValue
            self.assertEqual(expected, current_interval)

        # By default EdenFS currently automatically reloads
        # the config every 5 minutes
        default_interval = "5m"
        assert_current_interval(default_interval)

        # Tell EdenFS to reload the config file every 10ms
        write_user_config("10ms")

        # Make EdenFS reload the config immediately to get the new config values
        client.reloadConfig()
        assert_current_interval("10ms")

        # Update the reload interval again to 0ms.
        # This tells EdenFS not to auto-reload the config any more.
        write_user_config("0ms")

        # This change should be picked up automatically after ~10ms
        # Sleep for longer than this, then verify the config was updated.
        time.sleep(0.200)
        assert_current_interval("0ns")

        # Update the reload interval to 6ms.
        # This shouldn't be picked up automatically, since auto-reloads are disabled
        # right now.
        write_user_config("6ms")
        time.sleep(0.200)
        assert_current_interval("0ns")
        # Force this change to be picked up  now
        client.reloadConfig()
        assert_current_interval("6ms")

        # Update the reload interval again.
        # This should be picked up automatically again now that re-enabled the
        # periodic reload interval.
        write_user_config("7ms")
        time.sleep(0.200)
        assert_current_interval("7ms")

        # If we put a bogus value in the config file it should be ignored,
        # and the normal default (5 minutes) should be used.
        write_user_config("bogus value")
        time.sleep(0.200)
        assert_current_interval(default_interval)
