#!/usr/bin/env python

import os
import sys
import time
from optparse import OptionParser


if __name__ == "__main__":
    parser = OptionParser()
    parser.add_option(
        "--created",
        dest="created",
        type="string",
        default=[],
        action="append",
        help="wait for <FILE> to be created",
        metavar="FILE",
    )
    parser.add_option(
        "--deleted",
        dest="deleted",
        type="string",
        default=[],
        action="append",
        help="wait for <FILE> to be deleted",
        metavar="FILE",
    )
    parser.add_option(
        "--sleep-interval-ms",
        dest="sleep_interval_ms",
        type="int",
        default=100,
        help="time in MS to sleep between checks",
    )
    parser.add_option(
        "--max-time",
        dest="max_time",
        type="int",
        help="maximum time in seconds to wait for all the files to "
        "reach the desired state",
    )

    (options, args) = parser.parse_args()

    start = time.time()
    while options.max_time is None or time.time() < start + options.max_time:
        if not all(os.access(f, os.F_OK) for f in options.created) or any(
            os.access(f, os.F_OK) for f in options.deleted
        ):
            time.sleep(options.sleep_interval_ms / 1000.0)
            continue

        sys.exit(0)

    sys.exit(1)
