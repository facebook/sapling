#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import argparse
import unittest
import unittest.mock as mock

from eden.fs.cli.config import EdenInstance
from eden.fs.cli.configutil import EdenConfigParser
from eden.fs.cli.prefetch_profile import (
    DisablePredictiveProfileCmd,
    DisableProfileCmd,
    EnablePredictiveProfileCmd,
    EnableProfileCmd,
)


class PrefetchProfileTest(unittest.TestCase):
    def setUp(self) -> None:
        self.mock_eden_instance = mock.MagicMock(spec=EdenInstance)
        self.mock_argument_parser = mock.MagicMock(spec=argparse.ArgumentParser)
        self.mock_args = mock.MagicMock(spec=argparse.Namespace)

    @mock.patch("eden.fs.cli.prefetch_profile.get_eden_instance")
    def test_disable_no_config(self, mock_get_eden_instance: mock.MagicMock) -> None:
        mock_get_eden_instance.return_value = self.mock_eden_instance

        test_config = EdenConfigParser()
        self.mock_eden_instance.read_local_config.return_value = test_config

        test_disable = DisableProfileCmd(self.mock_argument_parser)
        test_disable.run(self.mock_args)

        self.mock_eden_instance.read_local_config.assert_called_once()
        self.mock_eden_instance.write_local_config.assert_called_once_with(test_config)
        self.assertEqual(
            test_config.get_section_str_to_any("prefetch-profiles"),
            {"prefetching-enabled": False},
        )

    @mock.patch("eden.fs.cli.prefetch_profile.get_eden_instance")
    def test_disable_existing_config(
        self, mock_get_eden_instance: mock.MagicMock
    ) -> None:
        mock_get_eden_instance.return_value = self.mock_eden_instance

        test_config = EdenConfigParser()
        test_config.read_dict(
            {
                "prefetch-profiles": {
                    "prefetching-enabled": True,
                },
                "something-random": {
                    "yup": "test",
                },
            }
        )
        self.mock_eden_instance.read_local_config.return_value = test_config

        test_disable = DisableProfileCmd(self.mock_argument_parser)
        test_disable.run(self.mock_args)

        self.mock_eden_instance.read_local_config.assert_called_once()
        self.mock_eden_instance.write_local_config.assert_called_once_with(test_config)
        self.assertEqual(
            test_config.get_section_str_to_any("prefetch-profiles"),
            {"prefetching-enabled": False},
        )
        self.assertTrue(test_config.has_section("something-random"))
        self.assertEqual(
            test_config.get_section_str_to_any("something-random"), {"yup": "test"}
        )

    @mock.patch("eden.fs.cli.prefetch_profile.get_eden_instance")
    def test_enable_no_config(self, mock_get_eden_instance: mock.MagicMock) -> None:
        mock_get_eden_instance.return_value = self.mock_eden_instance

        test_config = EdenConfigParser()
        self.mock_eden_instance.read_local_config.return_value = test_config

        test_enable = EnableProfileCmd(self.mock_argument_parser)
        test_enable.run(self.mock_args)

        self.mock_eden_instance.read_local_config.assert_called_once()
        self.mock_eden_instance.write_local_config.assert_called_once_with(test_config)
        self.assertEqual(
            test_config.get_section_str_to_any("prefetch-profiles"),
            {"prefetching-enabled": True},
        )

    @mock.patch("eden.fs.cli.prefetch_profile.get_eden_instance")
    def test_enable_existing_config(
        self, mock_get_eden_instance: mock.MagicMock
    ) -> None:
        mock_get_eden_instance.return_value = self.mock_eden_instance

        test_config = EdenConfigParser()
        test_config.read_dict(
            {
                "prefetch-profiles": {
                    "prefetching-enabled": False,
                },
                "something-random": {
                    "yup": "test",
                },
            }
        )
        self.mock_eden_instance.read_local_config.return_value = test_config

        test_enable = EnableProfileCmd(self.mock_argument_parser)
        test_enable.run(self.mock_args)

        self.mock_eden_instance.read_local_config.assert_called_once()
        self.mock_eden_instance.write_local_config.assert_called_once_with(test_config)
        self.assertEqual(
            test_config.get_section_str_to_any("prefetch-profiles"),
            {"prefetching-enabled": True},
        )
        self.assertTrue(test_config.has_section("something-random"))
        self.assertEqual(
            test_config.get_section_str_to_any("something-random"), {"yup": "test"}
        )

    @mock.patch("eden.fs.cli.prefetch_profile.get_eden_instance")
    def test_disable_predictive_no_config(
        self, mock_get_eden_instance: mock.MagicMock
    ) -> None:
        mock_get_eden_instance.return_value = self.mock_eden_instance

        test_config = EdenConfigParser()
        self.mock_eden_instance.read_local_config.return_value = test_config

        test_disable = DisablePredictiveProfileCmd(self.mock_argument_parser)
        test_disable.run(self.mock_args)

        self.mock_eden_instance.read_local_config.assert_called_once()
        self.mock_eden_instance.write_local_config.assert_called_once_with(test_config)
        self.assertEqual(
            test_config.get_section_str_to_any("prefetch-profiles"),
            {"predictive-prefetching-enabled": False},
        )

    @mock.patch("eden.fs.cli.prefetch_profile.get_eden_instance")
    def test_disable_predictive_existing_config(
        self, mock_get_eden_instance: mock.MagicMock
    ) -> None:
        mock_get_eden_instance.return_value = self.mock_eden_instance

        test_config = EdenConfigParser()
        test_config.read_dict(
            {
                "prefetch-profiles": {
                    "predictive-prefetching-enabled": True,
                },
                "something-random": {
                    "yup": "test",
                },
            }
        )
        self.mock_eden_instance.read_local_config.return_value = test_config

        test_disable = DisablePredictiveProfileCmd(self.mock_argument_parser)
        test_disable.run(self.mock_args)

        self.mock_eden_instance.read_local_config.assert_called_once()
        self.mock_eden_instance.write_local_config.assert_called_once_with(test_config)
        self.assertEqual(
            test_config.get_section_str_to_any("prefetch-profiles"),
            {"predictive-prefetching-enabled": False},
        )
        self.assertTrue(test_config.has_section("something-random"))
        self.assertEqual(
            test_config.get_section_str_to_any("something-random"), {"yup": "test"}
        )

    @mock.patch("eden.fs.cli.prefetch_profile.get_eden_instance")
    def test_enable_predictive_no_config(
        self, mock_get_eden_instance: mock.MagicMock
    ) -> None:
        mock_get_eden_instance.return_value = self.mock_eden_instance

        test_config = EdenConfigParser()
        self.mock_eden_instance.read_local_config.return_value = test_config

        test_enable = EnablePredictiveProfileCmd(self.mock_argument_parser)
        test_enable.run(self.mock_args)

        self.mock_eden_instance.read_local_config.assert_called_once()
        self.mock_eden_instance.write_local_config.assert_called_once_with(test_config)
        self.assertEqual(
            test_config.get_section_str_to_any("prefetch-profiles"),
            {"predictive-prefetching-enabled": True},
        )

    @mock.patch("eden.fs.cli.prefetch_profile.get_eden_instance")
    def test_enable_predictive_existing_config(
        self, mock_get_eden_instance: mock.MagicMock
    ) -> None:
        mock_get_eden_instance.return_value = self.mock_eden_instance

        test_config = EdenConfigParser()
        test_config.read_dict(
            {
                "prefetch-profiles": {
                    "predictive-prefetching-enabled": False,
                },
                "something-random": {
                    "yup": "test",
                },
            }
        )
        self.mock_eden_instance.read_local_config.return_value = test_config

        test_enable = EnablePredictiveProfileCmd(self.mock_argument_parser)
        test_enable.run(self.mock_args)

        self.mock_eden_instance.read_local_config.assert_called_once()
        self.mock_eden_instance.write_local_config.assert_called_once_with(test_config)
        self.assertEqual(
            test_config.get_section_str_to_any("prefetch-profiles"),
            {"predictive-prefetching-enabled": True},
        )
        self.assertTrue(test_config.has_section("something-random"))
        self.assertEqual(
            test_config.get_section_str_to_any("something-random"), {"yup": "test"}
        )
