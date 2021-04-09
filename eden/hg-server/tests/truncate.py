#!/usr/bin/env python

import argparse


if __name__ == "__main__":
    ap = argparse.ArgumentParser(usage="%(prog)s [options] file...")
    ap.add_argument(
        "-s",
        "--size",
        type=int,
        default=0,
        help="size in bytes to truncate to",
        metavar="BYTES",
    )
    ap.add_argument("file", nargs="+", help="file to truncate", metavar="FILE")
    args = ap.parse_args()

    size = args.size
    if size < 0:
        ap.error("size cannot be negative")
    for filename in args.file:
        with open(filename, "a+b") as f:
            f.truncate(size)
