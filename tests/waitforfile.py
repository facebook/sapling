#!/usr/bin/env python

from optparse import OptionParser
import os
import sys
import time

if __name__ == '__main__':
    parser = OptionParser()
    parser.add_option('--deleted', dest='deleted',
            type='string', default=[],
            action='append',
            help='wait for <FILE> to be deleted', metavar='FILE')
    parser.add_option('--sleep-interval-ms', dest='sleep_interval_ms',
            type='int', default=100,
            help='time in MS to sleep between checks')
    parser.add_option('--max-time', dest='max_time',
            type='int',
            help='maximum time in seconds to wait for all the files to '
                 'reach the desired state')

    (options, args) = parser.parse_args()

    start = time.time()
    while options.max_time is None or time.time() < start + options.max_time:
        for fpath in options.deleted:
            if os.access(fpath, os.F_OK):
                # still exists... :(
                break
        else:
            # nothing still exists, yay!
            sys.exit(0)

        time.sleep(options.sleep_interval_ms / 1000.0)

    sys.exit(1)
