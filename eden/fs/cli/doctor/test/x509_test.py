#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import datetime
import os
import sys
import time
import unittest
from pathlib import Path
from typing import Optional
from unittest.mock import patch

from eden.fs.cli.doctor.facebook import check_x509


@unittest.skipIf(sys.platform == "win32", "time.tzset is Unix-only")
class ValidateX509Test(unittest.TestCase):
    def setUp(self) -> None:
        self.saved_tz: Optional[str] = os.environ.get("TZ")

    def tearDown(self) -> None:
        if self.saved_tz is None:
            os.environ.pop("TZ", None)
        else:
            os.environ["TZ"] = self.saved_tz
        time.tzset()

    def validate_with_time_zone(
        self, time_zone: str, not_after: datetime.datetime
    ) -> str:
        os.environ["TZ"] = time_zone
        time.tzset()
        info = {"notAfter": not_after.strftime("%b %d %H:%M:%S %Y GMT")}
        with patch("ssl._ssl._test_decode_cert", return_value=info):
            return check_x509.validate_x509(Path(__file__))

    def test_certificate_valid_for_three_more_hours_east_of_utc(self) -> None:
        not_after = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(
            hours=3
        )
        result = self.validate_with_time_zone("Asia/Tokyo", not_after)
        self.assertTrue(result.startswith("x509 cert expired"), result)

    def test_certificate_expired_three_hours_ago_west_of_utc(self) -> None:
        not_after = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(
            hours=3
        )
        self.assertEqual(
            "", self.validate_with_time_zone("America/Los_Angeles", not_after)
        )
