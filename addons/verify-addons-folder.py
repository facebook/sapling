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
""",
        formatter_class=RawTextHelpFormatter,
    )
    parser.add_argument(
        "--use-vendored-grammars",
        help=(
            "Skips the codegen step for TextMate grammars that "
            + "fetches content from raw.githubusercontent.com. "
            + "Assumes TextMate codegen is already available."
        ),
        action="store_true",
    )
    args = parser.parse_args()
    asyncio.run(verify(use_vendored_grammars=args.use_vendored_grammars))


async def verify(*, use_vendored_grammars=False):
    await asyncio.gather(
        verify_prettier(),
        verify_shared(),
        verify_textmate(),
        verify_isl(),
        verify_reviewstack(use_vendored_grammars=use_vendored_grammars),
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


async def verify_textmate():
    timer = Timer("verifying textmate/")
    textmate = addons / "textmate"
    await asyncio.gather(
        run(["yarn", "run", "tsc", "--noEmit"], cwd=textmate),
        run(["yarn", "run", "eslint"], cwd=textmate),
    )
    timer.report(ok("textmate/"))


async def verify_isl():
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
    timer.report(ok("ISL"))


async def verify_reviewstack(*, use_vendored_grammars=False):
    timer = Timer("verifying reviewstack/")
    cwd = addons / "reviewstack"
    if use_vendored_grammars:
        # Normally, the full codegen step takes care of copying onig.wasm.
        src_onig_wasm = (
            addons / "node_modules" / "vscode-oniguruma" / "release" / "onig.wasm"
        )
        dest_onig_wasm = (
            addons
            / "reviewstack.dev"
            / "public"
            / "generated"
            / "textmate"
            / "onig.wasm"
        )
        shutil.copyfile(src_onig_wasm, dest_onig_wasm)
        await run(["yarn", "graphql"], cwd=cwd)
    else:
        await run(["yarn", "codegen"], cwd=cwd)
    await asyncio.gather(lint_and_test(cwd), verify_reviewstack_dev())
    timer.report(ok("reviewstack/"))


async def lint_and_test(cwd: Path):
    await asyncio.gather(
        run(["yarn", "run", "eslint"], cwd=cwd),
        run(["yarn", "test", "--watchAll=false"], cwd=cwd),
    )


async def verify_reviewstack_dev():
    """Requires codegen from reviewstack/ to have been built."""
    timer = Timer("verifying reviewstack.dev/")
    cwd = addons / "reviewstack.dev"
    await run(["yarn", "build"], cwd=cwd)
    await run(["yarn", "run", "eslint"], cwd=cwd)
    await run(["yarn", "release"], cwd=cwd)
    timer.report(ok("reviewstack.dev/"))


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
