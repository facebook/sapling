#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import asyncio
import shutil
import time
from argparse import RawTextHelpFormatter
from pathlib import Path
from typing import List

addons = Path(__file__).parent


def main():
    parser = argparse.ArgumentParser(
        description="""\
Verifies the contents of this folder by running tests and linters.

Requirements:
- `node` and `yarn` are on the `$PATH`
- `yarn install` has already been run in the addons/ folder
- `sl` required to be on the PATH for isl integration tests
""",
        formatter_class=RawTextHelpFormatter,
    )
    parser.add_argument(
        "--use-vendored-grammars",
        help=("No-op. Provided for compatibility."),
        action="store_true",
    )
    parser.add_argument(
        "--skip-integration-tests",
        help=("Don't run isl integrations tests"),
        action="store_true",
    )
    args = parser.parse_args()
    asyncio.run(verify(args))


async def verify(args):
    await asyncio.gather(
        verify_prettier(),
        verify_shared(),
        verify_components(),
        verify_textmate(),
        verify_isl(args),
        verify_internal(),
    )


async def verify_prettier():
    timer = Timer("verifying prettier")
    await run(["yarn", "run", "prettier-check"], cwd=addons)
    timer.report(ok("prettier"))


async def verify_shared():
    timer = Timer("verifying shared/")
    shared = addons / "shared"
    await lint_and_test(shared)
    timer.report(ok("shared/"))


async def verify_components():
    timer = Timer("verifying components/")
    components = addons / "components"
    await lint_and_test(components)
    timer.report(ok("components/"))


async def verify_textmate():
    timer = Timer("verifying textmate/")
    textmate = addons / "textmate"
    await asyncio.gather(
        run(["yarn", "run", "tsc", "--noEmit"], cwd=textmate),
        run(["yarn", "run", "eslint"], cwd=textmate),
    )
    timer.report(ok("textmate/"))


async def verify_internal():
    timer = Timer("verifying internal")
    await run(["yarn", "run", "verify-internal"], cwd=addons)
    timer.report(ok("internal"))


async def verify_isl(args):
    """Verifies isl/ and isl-server/ and vscode/ as the builds are interdependent"""
    timer = Timer("verifying ISL")
    isl = addons / "isl"
    isl_server = addons / "isl-server"
    vscode = addons / "vscode"

    await run(["yarn", "codegen"], cwd=isl_server)
    await asyncio.gather(
        run(["yarn", "build"], cwd=isl_server),
        run(["yarn", "build"], cwd=isl),
    )
    await asyncio.gather(
        run(["yarn", "build-extension"], cwd=vscode),
        run(["yarn", "build-webview"], cwd=vscode),
    )
    await asyncio.gather(
        lint_and_test(isl),
        lint_and_test(isl_server),
        lint_and_test(vscode),
    )
    if not args.skip_integration_tests:
        timer.report("running isl integration tests")
        # run integration tests separately to reduce flakiness from CPU contention with normal unit tests
        await run_isl_integration_tests()
    timer.report(ok("ISL"))


async def run_isl_integration_tests():
    await run(["yarn", "integration", "--watchAll=false"], cwd="isl")


async def lint_and_test(cwd: Path):
    await asyncio.gather(
        run(["yarn", "run", "eslint"], cwd=cwd),
        run(["yarn", "run", "tsc", "--noEmit"], cwd=cwd),
        run(["yarn", "test", "--watchAll=false"], cwd=cwd),
    )


async def run(args: List[str], cwd: str):
    process = await asyncio.create_subprocess_exec(
        *args, cwd=cwd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
    )
    stdout, stderr = await process.communicate()
    if process.returncode != 0:
        print(f"[stdout]\n{stdout.decode()}")
        print(f"[stderr]\n{stderr.decode()}")
        raise RuntimeError(f"command failed: {' '.join(args)}")


def ok(message: str) -> str:
    return f"\033[0;32mOK\033[00m {message}"


class Timer:
    def __init__(self, message):
        self._start = time.time()
        print(message)

    def report(self, message: str):
        end = time.time()
        duration = end - self._start
        print(f"{message} in {duration:.2f}s")


main()
