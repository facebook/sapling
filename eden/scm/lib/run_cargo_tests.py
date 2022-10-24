#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


import concurrent.futures
import glob
import os
import subprocess
import sys


def extraargs(manifestpath):
    """Extra CLI args for 'cargo test'"""
    args = []
    with open(manifestpath, "rb") as f:
        manifest = f.read()
    if b"fb =" in manifest:
        args += ["--features=fb"]
    return args


def runtest(manifestpath):
    cargo = os.getenv("CARGO", "cargo")
    name = os.path.dirname(manifestpath)
    try:
        os.unlink(os.path.join(name, "Cargo.lock"))
    except OSError:
        pass
    args = [cargo, "test", "-q", "--no-fail-fast"] + extraargs(manifestpath)
    try:
        subprocess.check_output(args, cwd=name, stderr=subprocess.PIPE)
        return None
    except subprocess.CalledProcessError as ex:
        if b"failures:" in ex.stdout:
            # Only show stdout which contains test failure information.
            # stderr might have warnings and are noisy.
            return (1, name, ex.stdout.decode("utf-8", "ignore"))
        elif b"linking with `cc` failed" in ex.stderr:
            # Could happen when python cannot be found.
            # Do not consider it as fatal.
            return (0, name, ex.stderr.decode("utf-8", "ignore"))
        elif b"LLVM ERROR: invalid" in ex.stderr:
            # ex. LLVM ERROR: invalid sh_type for string table section [index
            # 45]: expected SHT_STRTAB, but got SHT_NULL
            # Not fatal.
            return (0, name, ex.stderr.decode("utf-8", "ignore"))
        elif b"could not compile" in ex.stderr:
            # Only show stderr with information about how it fails
            # to compile.
            return (1, name, ex.stderr.decode("utf-8", "ignore"))
        else:
            # Could be flaky tests on Windows. For example:
            #
            #   warning: Error finalizing incremental compilation session directory `...`: Access is denied. (os error 5)
            #   warning: 1 warning emitted
            #   warning: Error finalizing incremental compilation session directory `...`: Access is denied. (os error 5)
            #   warning: 1 warning emitted
            #   error: test failed, to rerun pass '--lib'
            #   Caused by:
            #     process didn't exit successfully: `...\cpython_async-53e9eb016c0c638f.exe --quiet` (exit code: 0xc0000135, STATUS_DLL_NOT_FOUND)
            #
            #   running 1 test
            #   test src\stream.rs - stream::TStream (line 22) ... ok
            #
            #   test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
            #
            # Do not consider it as fatal.
            return (
                0,
                name,
                ex.stderr.decode("utf-8", "ignore")
                + ex.stdout.decode("utf-8", "ignore"),
            )


def runtests():
    manifestpaths = list(glob.glob("*/Cargo.toml"))
    details = []
    finalexitcode = 0
    write = sys.stdout.write
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as executor:
        futures = [executor.submit(runtest, p) for p in manifestpaths]
        for future in concurrent.futures.as_completed(futures):
            result = future.result()
            if result is None:
                continue
            code, name, output = result
            finalexitcode |= code
            # Only print failed tests.
            if code == 0:
                write(
                    "%s: test has non-zero exit code but isn't considered as failed\n"
                    % name
                )
            else:
                write("%s: test failed\n" % name)
            details.append("Details for %s:\n%s\n%s" % (name, output, "-" * 80))
    if details:
        write("\n\n%s" % "\n".join(details))
    sys.stdout.flush()
    return finalexitcode


if __name__ == "__main__":
    sys.exit(runtests())
