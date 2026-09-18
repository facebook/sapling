#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import unittest
import warnings
from unittest import mock

from eden.fs.service.eden.thrift_types import MountInfo, MountState
from parameterized import parameterized

from .lib import edenclient, testcase


class FuseTransportPolicyTest(unittest.TestCase):
    @parameterized.expand(
        [
            ("fallback_then_missing", ("devfuse", None), AssertionError),
            ("missing_then_fallback", (None, "devfuse"), AssertionError),
            ("fallback_then_unknown", ("devfuse", "unknown"), AssertionError),
            ("fallback_only", ("devfuse", "io_uring"), unittest.SkipTest),
        ]
    )
    def test_checks_all_mounts_before_skipping(
        self, name: str, transports: tuple[str | None, ...], error: type[Exception]
    ) -> None:
        mounts = [
            MountInfo(
                mountPoint=f"/mount{index}".encode(),
                edenClientPath=f"/client{index}".encode(),
                state=MountState.RUNNING,
                fsChannelType="fuse",
                fuseTransport=transport,
            )
            for index, transport in enumerate(transports)
        ]
        with self.assertRaises(error):
            edenclient.assert_fuse_transports(mounts, "io_uring")

    def test_matching_transport_passes(self) -> None:
        for transport in ("io_uring", "devfuse"):
            with self.subTest(transport=transport):
                self.assertIsNone(
                    edenclient.assert_fuse_transport(b"/mount", transport, transport)
                )

    def test_devfuse_fallback_skips_io_uring(self) -> None:
        with self.assertRaisesRegex(unittest.SkipTest, "/mount.*devfuse fallback"):
            edenclient.assert_fuse_transport(b"/mount", "io_uring", "devfuse")

    def test_other_mismatches_fail(self) -> None:
        for expected, actual in (
            ("io_uring", None),
            ("io_uring", "unknown"),
            ("devfuse", "io_uring"),
            ("devfuse", None),
        ):
            with self.subTest(expected=expected, actual=actual):
                with self.assertRaisesRegex(AssertionError, "/mount: expected"):
                    edenclient.assert_fuse_transport(b"/mount", expected, actual)


class IoUringTestMixinTest(unittest.TestCase):
    @parameterized.expand(
        [("none", None), ("empty", {}), ("cached", {"fuse": ["existing = true"]})]
    )
    def test_transport_config(
        self, name: str, extra_config: dict[str, list[str]] | None
    ) -> None:
        original = (
            {section: list(lines) for section, lines in extra_config.items()}
            if extra_config is not None
            else None
        )
        for enabled in (False, True):
            with self.subTest(io_uring=enabled):
                case = testcase.EdenTestCase()
                with (
                    mock.patch.object(case, "__unittest_skip__", False, create=True),
                    mock.patch.object(case, "init_eden_client"),
                    mock.patch.object(case, "report_time"),
                    mock.patch.object(case, "runTest", return_value=None, create=True),
                    mock.patch.object(case, "use_io_uring", return_value=enabled),
                    mock.patch.object(case, "set_rust_rollout_config"),
                    mock.patch.object(
                        case, "edenfs_extra_config", return_value=extra_config
                    ),
                    mock.patch.object(case, "write_configs") as write_configs,
                    mock.patch.object(testcase.sys, "platform", "linux"),
                    mock.patch.object(
                        testcase.os,
                        "uname",
                        return_value=mock.Mock(release="6.13.2-0_fbk7"),
                        create=True,
                    ),
                    warnings.catch_warnings(),
                ):
                    warnings.simplefilter("always", DeprecationWarning)
                    result = unittest.TestResult()
                    case.run(result)
                self.assertTrue(
                    result.wasSuccessful(), (result.failures, result.errors)
                )
                self.assertEqual([], result.skipped)
                self.assertEqual(original, extra_config)
                fuse_config = write_configs.call_args.args[0]["fuse"]
                self.assertEqual(
                    1, fuse_config.count(f"use-io-uring = {str(enabled).lower()}")
                )
                self.assertNotIn(
                    f"use-io-uring = {str(not enabled).lower()}", fuse_config
                )
                if original:
                    self.assertIn("existing = true", fuse_config)
                self.assertEqual(
                    enabled, "io-uring-pre-create-queues = true" in fuse_config
                )

    def test_selects_io_uring(self) -> None:
        self.assertFalse(testcase.EdenTestCase().use_io_uring())
        self.assertTrue(testcase.IoUringTestMixin().use_io_uring())
        self.assertFalse(issubclass(testcase.IoUringTestMixin, unittest.TestCase))

    @parameterized.expand(
        [
            ("6.13.2-0_fbk7", True),
            ("6.16.4-0_fbk1", True),
            ("6.13.2-generic", False),
            ("6.12.0_fbk1", False),
            ("6.17.0_fbk1", False),
        ]
    )
    def test_kernel_gate(self, release: str, supported: bool) -> None:
        class IoUringCase(testcase.IoUringTestMixin, testcase.EdenTestCase):
            pass

        case = IoUringCase()
        original_env = testcase.os.environ.get("EDENFS_INTEGRATION_TEST")
        with (
            mock.patch.object(testcase.sys, "platform", "linux"),
            mock.patch.object(
                testcase.os,
                "uname",
                return_value=mock.Mock(release=release),
                create=True,
            ),
            mock.patch.object(case, "__unittest_skip__", False, create=True),
            mock.patch.object(case, "runTest", return_value=None, create=True),
            mock.patch.object(case, "setup_eden_test") as setup,
        ):
            result = unittest.TestResult()
            case.run(result)
        self.assertTrue(result.wasSuccessful(), (result.failures, result.errors))
        self.assertEqual(
            original_env, testcase.os.environ.get("EDENFS_INTEGRATION_TEST")
        )
        self.assertEqual(not supported, bool(result.skipped))
        if supported:
            setup.assert_called_once()
        else:
            setup.assert_not_called()
            self.assertIn("requires an fbk", result.skipped[0][1])

    @parameterized.expand([("darwin",), ("win32",)])
    def test_non_linux_is_skipped_before_starting_eden(self, platform: str) -> None:
        with (
            mock.patch.object(testcase.sys, "platform", platform),
            mock.patch.object(testcase.EdenTestCase, "setup_eden_test") as setup,
        ):

            class IoUringCase(testcase.IoUringTestMixin, testcase.EdenTestCase):
                pass

            with self.assertRaisesRegex(unittest.SkipTest, "requires an fbk"):
                IoUringCase().setUp()
            setup.assert_not_called()
