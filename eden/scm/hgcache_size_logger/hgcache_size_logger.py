# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import logging
import os
import socket
import time

from libfb.py import log
from scubadata import ScubaData


def computecachesize(cachepath, logger):
    """measure size of cache directory"""
    skipped = 0

    cachesize = 0
    manifestsize = 0
    for root, dirs, files in os.walk(cachepath):
        dirsize = 0
        for filename in files:
            try:
                stat = os.lstat(os.path.join(root, filename))
                dirsize += stat.st_size
            except Exception as e:
                logger.warn(
                    ("error statting file '%s': %r. skipping file.\n") % (filename, e)
                )
                skipped += 1

        relpath = os.path.relpath(root, cachepath)
        segments = relpath.split(os.path.sep)
        if "manifests" in segments[1:]:
            manifestsize += dirsize
        else:
            cachesize += dirsize

    return (cachesize, manifestsize, skipped)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--hgcache-path", required=True, help="path to the hgcache location"
    )
    args = parser.parse_args()
    logger = log.set_simple_logging(level=logging.INFO)

    if not os.path.exists(args.hgcache_path):
        logger.error(f"path not found {args.hgcache_path}")
        return 0

    with ScubaData("mercurial_hgcache_size") as client:
        scuba_dict = {
            "normal": {"Host": socket.gethostname(), "Hgcache path": args.hgcache_path},
            "int": {"time": int(time.time())},
        }

        try:
            cachesize, manifestsize, skipped = computecachesize(
                args.hgcache_path, logger
            )
            scuba_dict["int"]["Cache size"] = cachesize
            scuba_dict["int"]["Manifest size"] = manifestsize
            scuba_dict["int"]["Skipped"] = skipped
        except Exception as exc:
            logger.exception("exception while computing cache size")
            scuba_dict["normal"]["error"] = str(exc)

        try:
            client.add_sample(scuba_dict)
        except Exception:
            logger.exception("exception while logging to scuba")


if __name__ == "__main__":
    main()
