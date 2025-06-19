# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import argparse
import logging
import os
import socket

from libfb.py import log
from rfe.scubadata.scubadata_py3 import Sample, ScubaData


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
        sample = Sample()
        # 'time' is implicitly set.
        # https://www.internalfb.com/wiki/Scuba/user_guide/Logging_to_Scuba/Using_ScubaData/#python
        sample.addNormalValue("Host", socket.gethostname())
        sample.addNormalValue("Hgcache path", args.hgcache_path)

        try:
            cachesize, manifestsize, skipped = computecachesize(
                args.hgcache_path, logger
            )
            sample.addIntValue("Cache size", cachesize)
            sample.addIntValue("Manifest size", manifestsize)
            sample.addIntValue("Skipped", skipped)
        except Exception as exc:
            logger.exception("exception while computing cache size")
            sample.addNormalValue("error", str(exc))

        try:
            client.addSample(sample)
        except Exception:
            logger.exception("exception while logging to scuba")


if __name__ == "__main__":
    main()
