#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import subprocess


with open("report.json", "r") as f:
    tests = json.load(f)
    for name, t in tests.items():
        name = name.split(" ")[0]
        if t["result"] == "skip":
            print("%s skipped" % name)
            subprocess.run("sed -i '/#require py2/d' %s" % name, shell=True)
            subprocess.run("sed -i '/require.*py2/d' %s" % name, shell=True)
