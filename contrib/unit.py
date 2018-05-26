#!/usr/bin/env python

"""run a subset of tests that related to the current change

Optionally write result using JSON format. The JSON format can be parsed
by MercurialTestEngine.php
"""
from __future__ import absolute_import

import json
import optparse
import os
import re
import subprocess
import sys

import utils


sys.path.insert(0, os.path.dirname(__file__))

reporoot = utils.reporoot


def info(message):
    """print message to stderr"""
    sys.stderr.write(message)


def checkoutput(*args, **kwds):
    """like subprocess.checked_output, but raise RuntimeError and return
    stderr as a second value.
    """
    proc = subprocess.Popen(
        *args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwds
    )
    out, err = proc.communicate()
    retcode = proc.poll()
    if retcode:
        raise RuntimeError("%r exits with %d" % (args, retcode))
    return out, err


def changedfiles(rev="wdir() + ."):
    """return a list of paths (relative to repo root) that rev touches.
    by default, check the working directory and its parent.
    """
    cmd = ["hg", "log", "-T", '{join(files,"\\0")}\\0', "-r", rev]
    out, err = checkoutput(cmd, cwd=reporoot)
    return set(out.rstrip("\0").split("\0"))


def words(path):
    """strip extension and split it to words.
    for example, 'a/b-c.txt' -> ['a', 'b', 'c']
    """
    return re.split("[^\w]+", os.path.splitext(path)[0])


def alltests(include_checks=True):
    tests = [
        p
        for p in os.listdir(os.path.join(reporoot, "tests"))
        if p.startswith("test-") and p[-2:] in ["py", ".t"]
    ]

    if not include_checks:
        tests = [t for t in tests if not t.startswith("test-check")]

    return tests


def interestingtests(changed_files, include_checks=True):
    """return a list of interesting test filenames"""
    tests = [
        p
        for p in os.listdir(os.path.join(reporoot, "tests"))
        if p.startswith("test-") and p[-2:] in ["py", ".t"]
    ]

    result = set()

    # Build a dictionary mapping words to the relevant tests
    # (tests whose name contains that word)
    testwords = {}
    for t in tests:
        # Include all tests starting with test-check*,
        if include_checks and t.startswith("test-check"):
            result.add(t)
            continue

        for word in words(t)[1:]:
            test_set = testwords.setdefault(word, set())
            test_set.add(t)

    # Also scan test files to check if they use extensions. For example,
    # pushrebase.py change should trigger test-pull-createmarkers.t since the
    # latter enables pushrebase extension.
    extre = re.compile("> ([^ ]+)\s*=\s*\$TESTDIR")
    for t in tests:
        with open(os.path.join(reporoot, "tests", t)) as f:
            content = f.read()
        for word in extre.findall(content):
            test_set = testwords.setdefault(word, set())
            test_set.add(t)

    # A test is interesting if there is a common word in both the path of the
    # changed source file and the name of the test file. For example:
    # - test-githelp.t is interesting if githelp.py is changed
    # - test-remotefilelog-sparse.t is interesting if sparse.py is changed
    # - test-remotefilelog-foo.t is interesting if remotefilelog/* is changed
    for path in changed_files:
        if path.startswith("tests/test-"):
            # for a test file, do not enable other tests but only itself
            result.add(os.path.basename(path))
            continue
        for w in words(path):
            result.update(testwords.get(w, []))

    return result


def runtests(tests=None):
    """run given tests

    Returns a tuple of (exitcode, report)
    exitcode will be 0 on success, and non-zero on failure
    report is a dictionary of test results.
    """
    modcheckpath = os.path.join(reporoot, "tests", "modcheck.py")
    args = ["-l", "--json", "--extra-config-opt=extensions.modcheck=%s" % modcheckpath]
    if tests:
        args += tests

    # Run the tests.
    #
    # We ignore KeyboardInterrupt while running the tests: when the user hits
    # Ctrl-C the interrupt will also be delivered to the test runner, which
    # should cause it to exit soon.  We want to wait for the test runner to
    # exit before we quit.  Otherwise may keep printing data even after we have
    # exited and returned control of the terminal to the user's shell.
    proc = utils.spawnruntests(args)
    interruptcount = 0
    maxinterrupts = 3
    while True:
        try:
            exitcode = proc.wait()
            break
        except KeyboardInterrupt:
            interruptcount += 1
            if interruptcount >= maxinterrupts:
                sys.stderr.write(
                    "Warning: test runner has not exited after "
                    "multiple interrupts.  Giving up on it and "
                    "quiting anyway.\n"
                )
                raise

    try:
        reportpath = os.path.join(reporoot, "tests", "report.json")
        with open(reportpath) as rf:
            report_contents = rf.read()

        # strip the "testreport =" header which makes the JSON illegal
        report = json.loads(re.sub("^testreport =", "", report_contents))
        os.unlink(reportpath)
    except (EnvironmentError, ValueError) as ex:
        # If anything goes wrong parsing the report.json file, build our own
        # fake failure report, and make sure we have non-zero exit code.
        sys.stderr.write("warning: error reading results: %s\n" % (ex,))
        report = {"run-tests": {"result": "failure"}}
        if exitcode == 0:
            exitcode = 1

    return exitcode, report


def main():
    op = optparse.OptionParser()
    op.add_option(
        "-j",
        "--json",
        metavar="FILE",
        help="Write a JSON result file at the specified location",
    )
    op.add_option("--all", action="store_true", default=False, help="Run all tests.")
    op.add_option(
        "--skip-checks",
        action="store_true",
        default=False,
        help='Do not automatically include all "check" tests.',
    )
    opts, args = op.parse_args()

    if opts.all:
        tests = alltests(include_checks=not opts.skip_checks)
    else:
        if args:
            changed_files = args
        else:
            changed_files = changedfiles()

        tests = interestingtests(changed_files, include_checks=not opts.skip_checks)

    if tests:
        info(
            "%d test%s to run: %s\n"
            % (len(tests), ("" if len(tests) == 1 else "s"), " ".join(tests))
        )
        exitcode, report = runtests(tests)
    else:
        info("no tests to run\n")
        exitcode = 0
        report = {}

    if opts.json:
        with open(opts.json, "w") as fp:
            json.dump(report, fp)
    return exitcode


if __name__ == "__main__":
    exitcode = main()
    sys.exit(exitcode)
