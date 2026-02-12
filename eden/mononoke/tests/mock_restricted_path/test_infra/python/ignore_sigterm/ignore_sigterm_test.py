#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict
import os
import signal
import sys
import time
import unittest


class IgnoreSigtermTest(unittest.TestCase):
    """
    Tests that help verify that the test runner sends a SIGTERM for graceful
    shutdown, but then hard kills the process.
    """

    def test_does_not_ignore_sigterm(self) -> None:
        if os.environ.get("TPX_PLAYGROUND_SLEEP") is not None:
            # pyre-fixme[6]: For 1st argument expected `Union[SupportsTrunc, str,
            #  SupportsIndex, SupportsInt, Buffer]` but got `Optional[str]`.
            time.sleep(int(os.environ.get("TPX_PLAYGROUND_SLEEP")))

    def test_ignores_sigterm(self) -> None:
        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        def ignore(a, b):
            sys.stdout.write("ignoring signal!\n")
            sys.stdout.flush()

        original_handler = signal.signal(signal.SIGTERM, ignore)
        if os.environ.get("TPX_PLAYGROUND_SLEEP") is not None:
            # pyre-fixme[6]: For 1st argument expected `Union[SupportsTrunc, str,
            #  SupportsIndex, SupportsInt, Buffer]` but got `Optional[str]`.
            time.sleep(int(os.environ.get("TPX_PLAYGROUND_SLEEP")))

        # Restore the signal handler to play nice
        signal.signal(signal.SIGTERM, original_handler)
