#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# A wrapper around the Sapling to improve the performance of the IntelliJ
# hg4idea plugin.
#
# Based on a community-maintained script at Meta. This is provided as an
# example. IntelliJ hg4idea compatibility is not officially supported by the
# Sapling team.
#
# Installation instructions:
# From IntelliJ, set the path to your project's Mercurial executable
# in "Preferences > Version Control > Mercurial" to this script.

import logging
import os
import re
import subprocess
import sys
import time
from logging.handlers import TimedRotatingFileHandler
from pathlib import Path


def setup_logging() -> None:
    logs_dir = Path.home() / ".jetbrains" / "hg4idea" / "logs"
    logs_dir.mkdir(parents=True, exist_ok=True)
    logger = logging.getLogger()
    logger.setLevel(logging.INFO)
    handler = TimedRotatingFileHandler(
        filename=os.path.join(logs_dir, "hg4idea_wrapper.log"),
        when="D",  # rotate daily
        interval=1,
        backupCount=14,  # keep two weeks' worth of logs
    )
    handler.setFormatter(logging.Formatter(fmt="%(levelname)s:%(asctime)s:%(message)s"))
    logger.addHandler(handler)


class BypassMercurialException(Exception):
    """Don't invoke Mercurial; use this stubbed behavior, instead."""

    def __init__(self, message, returncode=0, stdout=None, stderr=None):
        super(BypassMercurialException, self).__init__(message)
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr


# ----------------------------------------------------------------------
# This section contains "fixer" functions that each address a specific
# situational problem with the hg4idea's use of hg.  Each function is
# given an opportunity to process the hg command-line args, and it
# should do one of the following:
#   * Return None if the function doesn't apply to the invocation.
#   * Return modified args if the function attempts to improve it.
#   * Throws a BypassMercurialException if the improvement is that
#     Mercurial shouldn't be invoked at all.
# @hg_args_fixer is a convenience annotation that adds a fixer
# function to the list of applied fixes.
# ----------------------------------------------------------------------


HG_ARGS_FIXER_FUNCTIONS = []


def hg_args_fixer(f):
    HG_ARGS_FIXER_FUNCTIONS.append(f)
    return f


# hg4idea invokes `hg incoming` in the background regularly, which can take
# several minutes to complete.  Because the hg4idea plugin executes this
# command in the background, it doesn't *directly* block the IDE.  However,
# it wastes CPU time, and the next time hg4idea wants to execute any mutating
# hg command, it backs up that command behind the in-flight `hg incoming`,
# making it look like that *other* command is at fault for locking up the UI.
# This is painful, but avoidable, since Mercurial (essentially) only interprets
# the result as a boolean: are there changes, yes/no?
#
# Due to Facebook's scale, it's always highly likely that there's *some* change
# in the remote repo, so avoid the overhead of a real call by returning a fake
# commit claiming yes, there's some change out there.
@hg_args_fixer
def fake_incoming(args):
    """Mock `hg incoming` with a fake response."""
    args_iterator = iter(args)
    for arg in args_iterator:
        if arg == "--config" or arg == "--repository":
            next(args_iterator)  # next arg is for --flag, so skip it
        elif arg == "incoming":
            break
        else:
            return None  # Not an incoming command

    # Set default value for template from observation
    template = "{rev}\x17{node}\x17{author}\x17{desc|firstline}\x17\x03"
    for arg in args_iterator:
        if arg == "--config":
            next(args_iterator)  # next arg is --config flag arg, so skip it
        elif arg == "--template":
            template = next(args_iterator)
        elif arg == ["--"]:
            break  # Stop processing args

    raise BypassMercurialException(
        message="Skipped hg incoming",
        returncode=0,
        stdout=fake_commit_from_template(
            template,
            {
                "node": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                "rev": str(10**9),  # Some *absurdly* big number for a rev in a repo
                "author": "Mercurial Wrapper <%s@localhost>"
                % os.path.basename(__file__),
                "desc|firstline": "Fake commit to prevent hg4idea from hanging your IDE",
            },
        ),
    )


def fake_commit_from_template(template, fake_commit_values):
    keywords_used = set(re.findall(r"\{([^}]+)\}", template))
    for keyword in keywords_used:
        if keyword not in fake_commit_values:
            logging.error("Unrecognized keyword %s in template %s", keyword, template)
            fake_commit_values[keyword] = "{%s}" % keyword
    return template.format(**fake_commit_values)


# hg4idea invokes `hg outgoing` in the background regularly, which can take
# several minutes to complete.  Because the hg4idea plugin executes this
# command in the background, it doesn't *directly* block the IDE.  However,
# the next time hg4idea wants to execute any hg command, it has to queue up
# that command behind the `hg incoming` in flight, and so the *next* command
# potentially locks up the UI.
#
# To avoid costly negotiations with the server, assume that our local draft
# commits are a reasonable estimation of what hasn't been pushed to the server,
# and replace:
#    hg outgoing --newest-first
# with
#    hg log --rev 'sort(draft(), -date)'
#
# Implementation note: the use of '--newest-first' comes from:
# https://github.com/JetBrains/intellij-community/blob/master/plugins/hg4idea/src/org/zmlx/hg4idea/command/HgRemoteChangesetsCommand.java
@hg_args_fixer
def fake_outgoing(args):
    """Optimize `hg outgoing` with a fake response."""
    enumerated_args_iterator = iter(enumerate(args))
    for index, arg in enumerated_args_iterator:
        if arg == "--config" or arg == "--repository":
            next(enumerated_args_iterator)  # next arg is for --flag, so skip it
        elif arg == "outgoing":
            revisions = "draft()"
            rest = args[index + 1 :]
            if "--newest-first" in rest:
                rest.remove("--newest-first")
                args[index:] = rest
                revisions = "sort(%s,-date)" % revisions
            return args[:index] + ["log", "-r", revisions] + rest
        else:
            break
    return None  # Not an outgoing command


# hg4idea makes some template-based queries using the `file_copies` keyword,
# which can be *very* expensive to compute in large repos, especially when
# there are new files in a commit.  The `file_copies_switch` keyword is a
# lighterweight variety that doesn't calculate copies unless explicitly
# told to, as per:  https://www.mercurial-scm.org/repo/hg/help/templates
#
# Example request from hg4idea (truncated slightly for clarity):
# hg log --template "{join(file_adds,'\x01')}0x17{join(file_copies,'\x01')}\x03"
# Becomes:
# hg log --template "{join(file_adds,'\x01')}0x17{join(file_copies_switch,'\x01')}\x03"
@hg_args_fixer
def fix_file_copies(args):
    """Rewrites --templates to avoid using `file_copies`."""
    enumerated_args_iterator = iter(enumerate(args))
    for _, arg in enumerated_args_iterator:
        if arg == "--template":
            # The next argument is a template. If it contains the expensive
            # `file_copies` macro, replace it with cheaper `file_copies_switch`
            index, arg = next(enumerated_args_iterator)
            fixed = re.sub(r"\bfile_copies\b", "file_copies_switch", arg)
            if fixed == arg:
                return None  # No change
            return args[:index] + [fixed] + args[index + 1 :]
        elif arg == "--":
            break  # anything after '--' should not be processed as a flag
    return None


# Avoid unnecessary status calls about ignored files. For example,
# hg4idea goes through the buck-out directory and asks hg about the
# status of each one.  Individually, these calls aren't prohibitively
# expensive (about a second of wall time), but in total, IntelliJ can
# waste several minutes walking the entirety of the buck-out directory.
@hg_args_fixer
def avoid_status_for_ignored_files(args):
    """Avoid calling status on files in ignored folders (e.g. buck-out)."""
    # Scan up to the list of files
    args_before_files = []
    enumerated_args_iterator = iter(enumerate(args))
    for index, arg in enumerated_args_iterator:
        if arg == "--config" or arg == "--repository":
            next(enumerated_args_iterator)  # next arg is for --flag, so skip it
            continue
        if arg == "status":
            status_flags = {
                "--added",
                "--modified",
                "--removed",
                "--deleted",
                "--unknown",
                "--copies",
            }
            flags_start = index + 1  # one step past the 'status' command
            flags_end = flags_start + len(status_flags)
            if set(args[flags_start:flags_end]) == status_flags:
                args_before_files = args[:flags_end]
        break
    if not args_before_files:
        return None
    files = []
    files_skipped = False
    args_after_files = []
    args_iterator = iter(args[len(args_before_files) :])
    for arg in args_iterator:
        if arg.startswith("--"):
            args_after_files = [arg] + list(args_iterator)
            break
        if (
            arg.startswith(".idea/")
            or arg.startswith(".eden/")
            or arg.startswith("gradleBuild/")
        ):
            files_skipped = True
        else:
            files.append(arg)
    if files_skipped:
        if len(files) == 0:
            raise BypassMercurialException(
                "Returning no status: all files are ignored."
            )
        return args_before_files + files + args_after_files

    return None


# hg4idea sometimes asks for the status of the entire repo using the
# `--ignored` flag, which is particularly problematic, since that
# forces hg to bypass watchman and go directly to the filesystem,
# causing absolutely horrible performance.
#
# This is a multi-fix which patches several different whole-repo status calls:
#
# (1)  org.zmlx.hg4idea.HgVFSListener uses status to find all --unknown and
# --ignored files, to potentially add files to a commit.  Hg can return
# --unknown files quickly, thanks to watchman, but --ignored files require a
# crawl of the filesystem, and are almost *never* what the user really wants
# (after all, the files are supposed to be ignored in the first place).  This
# is not correct, but the performance is *so* much better as to make it a
# tolerable tradeoff.
# TODO: If the repo is checked out using Eden, (1) is not problematic
#
# (2)  By org.zmlx.hg4idea.provider.HgLocalIgnoredHolder, which gives it
# 500 milliseconds to finish (and if it doesn't, seems to ignore the result).
# Since this method cannot finish correctly in 500 milliseconds, sacrifice
# correctness for speed by pretending that there are no ignored files.
# TODO: Fix (2) by setting hg4idea.process.ignored in the IntelliJ registry
@hg_args_fixer
def restrict_hg_status_ignored(args):
    """Restricts hg status calls that would trigger large filesystem recrawls."""
    enumerated_args_iterator = iter(enumerate(args))
    index = 0
    for index, arg in enumerated_args_iterator:  # noqa B007
        if arg == "--config" or arg == "--repository":
            next(enumerated_args_iterator)  # next arg is for --flag, so skip it
        elif arg == "status":
            break  # continue
        else:
            return None

    # Convert : hg status --unknown --ignored --encoding UTF-8
    # to      : hg status --unknown --encoding UTF_8
    if args[index + 1 :] == ["--unknown", "--ignored", "--encoding", "UTF-8"]:
        args[index + 1 :] = ["--unknown", "--encoding", "UTF-8"]
        return args

    # Return nothing for hg status --ignored
    if args[index + 1] == "--ignored":
        # Return failure (returncode=1) to indicate that the results shouldn't be trusted
        raise BypassMercurialException(
            "Returning no status for ignored files.", returncode=1
        )

    return None


# hg4idea uses 'status --copies' to show changes when annotating files.
# Using --copies is very slow and would result in waiting for minutes
# to get status result for a particular file. Let's remove it to speed
# this command up.
@hg_args_fixer
def remove_copies_flag_from_status(args):
    if ("status" in args) and ("--copies" in args):
        args.remove("--copies")

    return args


# hg4idea will occasionally issue unbounded log calls, walking the
# entire history of the repo.  If it tries to do that, limit it to
# an arbitrarily large but not ridiculous number of entries.
#
# Example:
#    hg log
# Becomes:
#    hg log --limit 100000
# But this is unchanged, since it deliberately has a revision specification:
#    hg log -r .
@hg_args_fixer
def limit_log_length(args, max_length=100_000):
    """Prevents unbounded hg log calls by inserting `--limit` arg."""
    enumerated_args_iterator = iter(enumerate(args))
    for index, arg in enumerated_args_iterator:
        if arg == "--config" or arg == "--repository":
            next(enumerated_args_iterator)  # next arg is for --flag, so skip it
        elif arg == "log":
            if "-r" in args[index:] or "--rev" in args[index:]:
                break  # ok, because log has a revision specification
            if "--limit" in args[index:]:
                break  # ok, because log is already limited
            return args + ["--limit", str(max_length)]
        else:
            break
    return None


# Revision format '123456:deadbeef' is no longer supported,
@hg_args_fixer
def fix_revision_number_format(args):
    """
    Convert the revision format from:
    '123456:deadbeef' or '123456::deadbeef'
    to:
    'deadbeef'
    """
    for index, arg in enumerate(args):
        next_index = index + 1
        if (
            arg == "--rev"
            and next_index in range(0, len(args))
            and str(args[next_index]).count(":") in [1, 2]
        ):
            args[next_index] = str(args[next_index]).split(":")[-1]
    return args


# hg4idea invokes `hg version -q` and expects a certain format of output.
# See https://github.com/JetBrains/intellij-community/blob/master/plugins
# /hg4idea/src/org/zmlx/hg4idea/util/HgVersion.java
@hg_args_fixer
def fake_version(args):
    if "version" not in args[0:1]:
        # not a 'version' command
        return None
    raise BypassMercurialException(
        message="fixed hg version format",
        returncode=0,
        stdout="Mercurial Distributed SCM (version 4.4.2)\n",
    )


def _replace_rev_expression_with_mod(arg, rev_expr):
    """rev_expr can be rev, p1rev or p2rev."""
    upper_bound = 2147483648
    return arg.replace(
        "{" + rev_expr + "}",
        "{" + f"ifgt({rev_expr}, 0, mod({rev_expr}, {upper_bound}), {rev_expr})" + "}",
    )


@hg_args_fixer
def change_rev_to_32_bit_int(args):
    """
    Hg's rev numbers are in the 64-bit range which makes hg4idea throw
    exceptions while trying to parse the number as a 32-bit integer.
    """
    rev_exprs = ["rev", "p1rev", "p2rev"]
    new_args = []
    for arg in args:
        for rev_expr in rev_exprs:
            arg = _replace_rev_expression_with_mod(arg, rev_expr)
        new_args.append(arg)
    return new_args


@hg_args_fixer
def replace_parents(args):
    """
    Android Studio issues 'parents' parameter for some workflows (f.e. when resolving a merge conflict).
    However the 'parents' is no longer supported, and should be replaced with 'log --limit 1' instead.
    """

    phase1_args = []
    strip_file_path = False
    for arg in args:
        if "parents" == arg:
            phase1_args.extend(["log", "-r", "p1()+p2()"])
            strip_file_path = True
        else:
            phase1_args.append(arg)

    # Android studio uses `hg parents /path/to/file`, but there is no need to specify a file since
    # the value with or without file specified will be the same, but performance would be faster
    # when file is not specified.
    phase2_args = []
    if strip_file_path:
        next_arg_is_flag_parameter = False
        for arg in phase1_args:
            if arg.startswith("-"):
                next_arg_is_flag_parameter = True
                phase2_args.append(arg)
            elif next_arg_is_flag_parameter:
                next_arg_is_flag_parameter = False
                phase2_args.append(arg)
            elif not arg.startswith("/"):
                phase2_args.append(arg)

    return phase2_args


# ----------------------------------------------------------------------
# End section of "fixer" functions
# ----------------------------------------------------------------------


def execute_hg(args, original_args):
    logging.info(
        "Running hg command: %s (original: %s) from working dir %s",
        args,
        original_args,
        os.path.abspath(os.path.curdir),
    )
    start = time.time()
    result_length = 0
    sl = os.getenv("SL") or "sl"
    p = subprocess.Popen([sl] + args, stdout=subprocess.PIPE)
    try:
        stdout, _ = p.communicate()
        sys.stdout.buffer.write(stdout)
        sys.stdout.buffer.flush()
        result_length += len(stdout)
    finally:
        if p.returncode is None:
            p.kill()
        finish = time.time()
        log_function = logging.info
        elapsed_time = finish - start
        if elapsed_time > 30 or result_length > 10_000_000:
            log_function = logging.warning
        log_function(
            "Result[retcode=%s]: %d bytes in %0.2f sec for %s (original: %s)",
            str(p.returncode),
            result_length,
            elapsed_time,
            args,
            original_args,
        )
    return 1 if p.returncode is None else p.returncode


def do_modified_mercurial(
    original_args, fixer_functions=HG_ARGS_FIXER_FUNCTIONS, executor=execute_hg
):
    logging.debug(
        "Original command: %s (working dir: %s)",
        original_args,
        os.path.abspath(os.path.curdir),
    )
    try:
        args = [str(arg) for arg in original_args]
        for f in fixer_functions:
            new_args = f(args)
            if new_args:
                # Uncomment this for troubleshooting.
                # logging.info("Args changed by %s, now %s", f.__name__, new_args)
                logging.debug("%s: %s", f.__name__, f.__doc__)
                args = new_args
    except BypassMercurialException as e:
        logging.info(
            "Command bypassed, returncode %d: %s (original: %s)",
            e.returncode,
            str(e),
            original_args,
        )
        if e.stdout:
            sys.stdout.write(e.stdout)
            sys.stdout.flush()
        if e.stderr:
            sys.stderr.write(e.stderr)
            sys.stderr.flush()
        return e.returncode
    except Exception as e:
        logging.exception(f"Unexpected exception parsing: {original_args}: {e}")
        return 1
    return executor(args, original_args)


def main():
    setup_logging()
    returncode = do_modified_mercurial(sys.argv[1:])
    sys.exit(returncode)


if __name__ == "__main__":
    main()
