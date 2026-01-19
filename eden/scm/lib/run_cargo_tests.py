#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import argparse
import concurrent.futures
import glob
import json
import os
import subprocess
import sys
import textwrap
from typing import List, Optional, Tuple


def manifestargs(manifestpath: str) -> List[str]:
    """Extra CLI args for 'cargo test'"""
    args = []
    with open(manifestpath, "rb") as f:
        manifest = f.read()
    if b"fb =" in manifest:
        args += ["--features=fb"]
    return args


def runtest(
    manifestpath: str, extraargs: Optional[List[str]] = None
) -> Tuple[int, str, str]:
    """Run a test given the Cargo.toml path.
    Return (exit_code, description, cargo_output).
    exit_code:
        0: passed
        1: failed
        2: failed but not fatal, like flaky "Access is denied" on Windows, lack
           of OpenSSL dependency, etc.
    """
    cargo = os.getenv("CARGO", "cargo")
    dirname = os.path.dirname(manifestpath)
    name = describeruntestargs(manifestpath, extraargs)
    try:
        os.unlink(os.path.join(dirname, "Cargo.lock"))
    except OSError:
        pass
    args = (
        [cargo, "test", "-q", "--no-fail-fast"]
        + manifestargs(manifestpath)
        + (extraargs or [])
    )
    try:
        subprocess.check_output(args, cwd=dirname, stderr=subprocess.PIPE)
        return (0, name, "")
    except subprocess.CalledProcessError as ex:
        output = ex.stderr.decode("utf-8", "ignore") + ex.stdout.decode(
            "utf-8", "ignore"
        )
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
        elif "file not found for module" in output and "out" in output:
            # Thrift codegen issue. Ignore for now.
            # error[E0583]: file not found for module `mock`
            #  --> ...\build\cargo-target\debug\build\fb303_core_clients-69ca1b52c8a46089\out\lib.rs:9:1
            #   |
            # 9 | pub mod mock;
            #   | ^^^^^^^^^^^^^
            #   |
            return (0, name, output)
        elif "::fbthrift" in output:
            # Thrift related issue. Ignore for now.
            message = (
                "fbthrift-related compilation error is ignored:\n%s"
                % shorten_error_message(output)
            )
            return (2, name, message)
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
                output,
            )


def shorten_error_message(message: str) -> str:
    r"""Shorten error message. Pick up 2 "error" blocks.

    This can be useful when the error is super long (ex. 2k lines) because of a
    mismatched dependency somehow.

    >>> m = r'''warning: unnecessary `unsafe` block
    ...   --> eden\scm\lib\util\src\file.rs:29:51
    ...    |
    ... 29 | pub(crate) static UMASK: Lazy<u32> = Lazy::new(|| unsafe {
    ...    |                                                   ^^^^^^ unnecessary `unsafe` block
    ...    |
    ...    = note: `#[warn(unused_unsafe)]` on by default
    ...
    ... error[E0407]: method `write` is not a member of trait `fbthrift::Serialize`
    ...    --> eden\scm\build\cargo-target\debug\build\config_thrift-b6a866cf8706c848\out\lib.rs:155:5
    ...     |
    ... 155 | /     fn write(&self, p: &mut P) {
    ... 156 | |         p.write_i32(self.into())
    ... 157 | |     }
    ...     | |_____^ not a member of trait `fbthrift::Serialize`
    ...
    ... error[E0407]: method `read` is not a member of trait `fbthrift::Deserialize`
    ...    --> eden\scm\build\cargo-target\debug\build\config_thrift-b6a866cf8706c848\out\lib.rs:165:5
    ...     |
    ... 165 | /     fn read(p: &mut P) -> ::anyhow::Result<Self> {
    ... 166 | |         ::std::result::Result::Ok(Self::from(::anyhow::Context::context(p.read_i32(), "Expected a number indicating enum variant")?))
    ... 167 | |     }
    ...     | |_____^ not a member of trait `fbthrift::Deserialize`
    ...
    ... error[E0407]: method `write` is not a member of trait `fbthrift::Serialize`
    ...    --> eden\scm\build\cargo-target\debug\build\config_thrift-b6a866cf8706c848\out\lib.rs:275:5
    ...     |
    ... 275 | /     fn write(&self, p: &mut P) {
    ... 276 | |         p.write_i32(self.into())
    ... 277 | |     }
    ...     | |_____^ not a member of trait `fbthrift::Serialize`
    ...
    ... error[E0407]: method `read` is not a member of trait `fbthrift::Deserialize`
    ...    --> eden\scm\build\cargo-target\debug\build\config_thrift-b6a866cf8706c848\out\lib.rs:285:5
    ...     |
    ... 285 | /     fn read(p: &mut P) -> ::anyhow::Result<Self> {
    ... 286 | |         ::std::result::Result::Ok(Self::from(::anyhow::Context::context(p.read_i32(), "Expected a number indicating enum variant")?))
    ... 287 | |     }
    ...     | |_____^ not a member of trait `fbthrift::Deserialize`
    ...
    ... Other message 1
    ... Other message 2'''
    >>> print(shorten_error_message(m))
    ... [omitted 8 lines] ...
    error[E0407]: method `write` is not a member of trait `fbthrift::Serialize`
       --> eden\scm\build\cargo-target\debug\build\config_thrift-b6a866cf8706c848\out\lib.rs:155:5
        |
    155 | /     fn write(&self, p: &mut P) {
    156 | |         p.write_i32(self.into())
    157 | |     }
        | |_____^ not a member of trait `fbthrift::Serialize`
    <BLANKLINE>
    error[E0407]: method `read` is not a member of trait `fbthrift::Deserialize`
       --> eden\scm\build\cargo-target\debug\build\config_thrift-b6a866cf8706c848\out\lib.rs:165:5
        |
    165 | /     fn read(p: &mut P) -> ::anyhow::Result<Self> {
    166 | |         ::std::result::Result::Ok(Self::from(::anyhow::Context::context(p.read_i32(), "Expected a number indicating enum variant")?))
    167 | |     }
        | |_____^ not a member of trait `fbthrift::Deserialize`
    <BLANKLINE>
    ... [omitted 18 lines] ...
    <BLANKLINE>
    """
    important_lines = []
    is_important = False
    error_count = 0
    omitted = 0

    def append_omitted():
        nonlocal omitted
        if omitted:
            important_lines.append(f"... [omitted {omitted} lines] ...\n")
        omitted = 0

    lines = message.splitlines(True)
    for line in lines:
        if not is_important and line.startswith("error[E") and error_count < 2:
            error_count += 1
            is_important = True
            append_omitted()
        if is_important:
            important_lines.append(line)
        else:
            omitted += 1
        if is_important and not line.strip():
            is_important = False
    append_omitted()

    if not important_lines:
        # Just truncate the lines
        threshold = 20
        important_lines += lines[:threshold]
        omitted = max(0, len(lines) - threshold)
        append_omitted()

    return "".join(important_lines)


def describeruntestargs(
    manifestpath: str, extraargs: Optional[List[str]] = None
) -> str:
    """Describe parameter that will be passed to runtest"""
    name = os.path.dirname(manifestpath)
    if extraargs:
        return f"{name} {' '.join(extraargs)}"
    else:
        return name


def indent(lines: List[str]) -> str:
    return textwrap.indent("\n".join(lines), "  ")


def extractfeatures(content: str) -> List[str]:
    """Extract cargo features from Cargo.toml"""
    try:
        import tomllib

        obj = tomllib.loads(content)
        return obj.get("features", {}).get("default") or []

    except ImportError:
        # Python < 3.11. Naive, incorrect, but good enough practically.
        for line in content.splitlines():
            if line.startswith("default = ["):
                return json.loads(line.split(" = ", 1)[-1])
        return []


def getruntestargs(manifestpath: str) -> List[Tuple[str, Optional[List[str]]]]:
    result = [(manifestpath, None)]
    with open(manifestpath) as f:
        content = f.read()
    features = extractfeatures(content)
    if features:
        # In theory we need to test 2 ** len(features) cases to cover
        # everything. But that could be too many. Let's just test that:
        # - all features turned off
        # - turning on only one feature
        result += [(manifestpath, ["--no-default-features"])]
        result += [
            (manifestpath, ["--no-default-features", f"--features={feature}"])
            for feature in features
        ]
    return result


def runtests(names: List[str], verbose: bool = False, jobs: int = 1) -> int:
    """Run all tests in parallel. Return exit code"""
    manifestpaths = list(glob.glob("*/Cargo.toml"))
    if names:
        manifestpaths = [
            path for path in manifestpaths if any(name in path for name in names)
        ]

    runtestargs = [args for path in manifestpaths for args in getruntestargs(path)]
    details = []
    finalexitcode = 0
    write = sys.stdout.write

    if verbose:
        write(
            f"Running tests for:\n{indent([describeruntestargs(*args) for args in runtestargs])}\n"
        )

    passed = []
    failed = []
    ignored = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as executor:
        futures = [executor.submit(runtest, *args) for args in runtestargs]
        for future in concurrent.futures.as_completed(futures):
            result = future.result()
            code, name, output = result
            if code == 0:
                if verbose:
                    write(f"{name}: passed\n")
                passed.append(name)
                continue
            assert code in (1, 2)
            finalexitcode |= 2 - code
            # Print failures immediately.
            if code == 2:
                write(
                    f"{name}: test has non-zero exit code but isn't considered as failed\n"
                )
                ignored.append(name)
            else:
                write(f"{name}: test failed\n")
                failed.append(name)
            # Print cargo output later.
            details.append("Details for %s:\n%s\n%s" % (name, output, "-" * 80))
    if details:
        write("\n\n%s" % "\n".join(details))

    # Summary
    write("-" * 70)
    if passed:
        write(f"\nPassed:\n{indent(passed)}\n")
    if ignored:
        write(f"\nNon-fatal failures:\n{indent(ignored)}\n")
    if failed:
        write(f"\nFailed:\n{indent(failed)}\n")
    sys.stdout.flush()

    return finalexitcode


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Process some integers.")
    parser.add_argument("names", nargs="*", help="crate names to test")
    parser.add_argument(
        "-v", "--verbose", action="store_true", help="print verbose output"
    )
    parser.add_argument("-j", "--jobs", default=3, help="run tests in parallel")
    args = parser.parse_args()
    sys.exit(runtests(names=args.names, verbose=args.verbose, jobs=args.jobs))
