#!/usr/bin/env python2
# stresstest-atomicreplace.py - test interrupting threading.Condition
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# This stress test checks if the replace logic in Mercurial is really atomic
import argparse
import os
import random
import shutil
import subprocess
import tempfile
import time


CONTENT = "Aeneas was a lively fellow"
FILENAME = "hg.stresstest.atomicreplace.file"
CMD = """%s debugshell -c "from edenscm.mercurial.util import atomictempfile; f=atomictempfile('%s'); f.write('%s'); f.close()" """


def run_stress_test(n, binary, kill_median, kill_half_width):
    content = CONTENT
    tempdir = tempfile.mkdtemp(prefix="hg.stresstest.dir")
    filename = os.path.join(tempdir, FILENAME)
    py_filename = filename.replace("\\", "\\\\")
    min_sleep = (kill_median - kill_half_width) / 1000.0
    max_sleep = (kill_median + kill_half_width) / 1000.0
    print "Will run (file content changes between iterations):"
    print "  ", CMD % (binary, py_filename, content)
    print "for %i times and kill each run after unform(%f..%f) s" % (
        n,
        min_sleep,
        max_sleep,
    )
    print "Will check the existense of %s after each run\n" % filename
    try:
        # let's create a file upfront
        open(filename, "w").close()
        for p in xrange(n):
            content += "!"
            proc = subprocess.Popen(CMD % (binary, py_filename, content))
            tosleep = random.uniform(min_sleep, max_sleep)
            time.sleep(tosleep)
            proc.kill()
            if not os.path.exists(filename):
                print u"ALARM! Iteration %i failed. File not found. Slept: %fs" % (
                    p,
                    tosleep,
                )
    finally:
        shutil.rmtree(tempdir)


if __name__ == "__main__":
    if os.name == "nt":
        default_binary = os.path.realpath("../hg.exe")
    else:
        default_binary = os.path.realpath("../hg.rust")

    desc = (
        "Stress-test the atomic replace logic in Mercurial.\n\n "
        + "This script runs N iterations of hg binary, asking it to "
        + "atomically create a file with some content. It kills each "
        + "process after a random period of time. After the process is"
        + "killed, the script checks the existense of the file and "
        + "reports if it is missing."
    )
    parser = argparse.ArgumentParser(
        description=desc, formatter_class=argparse.ArgumentDefaultsHelpFormatter
    )
    parser.add_argument(
        "-n", type=int, default=200, help="a number of iterations to run"
    )
    parser.add_argument(
        "-k",
        type=int,
        default=450,
        help="an average time after which to kill the hg "
        "process in ms (kills will be in +/-D ms "
        "interval from from this time)",
    )
    parser.add_argument(
        "-w",
        type=int,
        default=50,
        help="half a width of uniform distribution of " "kill times (ms)",
    )
    parser.add_argument(
        "-b",
        type=str,
        default=default_binary,
        help="a path to a mercurial binary to stress-test",
    )
    args = parser.parse_args()
    run_stress_test(args.n, args.b, args.k, args.w)
