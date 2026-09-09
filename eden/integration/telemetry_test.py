#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from eden.fs.config.eden_config.thrift_types import (
    ConfigReloadBehavior,
    ConfigSourceType,
)
from eden.fs.service.eden.thrift_types import (
    CheckoutMode,
    CheckOutRevisionParams,
    GetConfigParams,
)

from .lib import testcase

ENABLE_SCRIBE_LOGGING = "telemetry:enable-scribe-logging"
XPLAT_ENQUEUED_COUNTER = "telemetry.xplat_messages_enqueued.sum"


@testcase.eden_repo_test
class TelemetryTest(testcase.EdenRepoTest):
    """
    The test harness must keep the daemon it spawns out of the production
    Scuba tables. These tests pin that down so a harness change that drops the
    setting, or a daemon change that stops honoring it, fails loudly here
    instead of showing up as noise in perfpipe_edenfs_events.
    """

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.commit1 = self.repo.commit("Initial commit.")
        self.repo.write_file("hello", "adios\n")
        self.commit2 = self.repo.commit("Second commit.")

    async def test_harness_disables_scribe_logging(self) -> None:
        async with self.get_async_thrift_client() as client:
            config = await client.getConfig(
                GetConfigParams(reload=ConfigReloadBehavior.NoReload)
            )
        value = config.values.get(ENABLE_SCRIBE_LOGGING)
        self.assertIsNotNone(value)
        assert value is not None
        self.assertEqual("false", value.parsedValue)
        self.assertEqual(ConfigSourceType.SystemConfig, value.sourceType)

    async def test_no_scuba_samples_are_enqueued(self) -> None:
        # A checkout always logs a FinishedCheckout event to edenfs_events, so
        # after one the enqueue counter is the observable proof that the gate
        # dropped the sample before it reached the XplatLogger queue.
        async with self.get_async_thrift_client() as client:
            await client.checkOutRevision(
                mountPoint=self.mount_path_bytes,
                snapshotHash=self.commit1.encode(),
                checkoutMode=CheckoutMode.NORMAL,
                params=CheckOutRevisionParams(),
            )
        self.assertEqual("hola\n", self.read_file("hello"))

        counters = self.get_counters()
        self.assertEqual(0, counters.get(XPLAT_ENQUEUED_COUNTER, 0))
