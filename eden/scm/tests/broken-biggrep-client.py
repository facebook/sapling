#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This is a terribly anemic fake implementation of the biggrep client
import argparse
import sys


parser = argparse.ArgumentParser()
parser.add_argument("--stripdir", action="store_true")
parser.add_argument("-r", action="store_true")
parser.add_argument("--color")
parser.add_argument("--expression")
parser.add_argument("-f")
parser.add_argument("tier")
parser.add_argument("corpus")
parser.add_argument("engine")
args = parser.parse_args()

print("broken biggrepclient", file=sys.stderr)
sys.exit(2)
