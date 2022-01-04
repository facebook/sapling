#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import argparse
import os
from pathlib import Path
from typing import Optional, Tuple, Union

from . import config as config_mod, subcmd as subcmd_mod
from .config import EdenCheckout, EdenInstance


def get_eden_instance(args: argparse.Namespace) -> EdenInstance:
    return EdenInstance(
        args.config_dir, etc_eden_dir=args.etc_eden_dir, home_dir=args.home_dir
    )


def find_checkout(
    args: argparse.Namespace, path: Union[Path, str, None]
) -> Tuple[EdenInstance, Optional[EdenCheckout], Optional[Path]]:
    if path is None:
        path = os.getcwd()
    return config_mod.find_eden(
        path,
        etc_eden_dir=args.etc_eden_dir,
        home_dir=args.home_dir,
        state_dir=args.config_dir,
    )


def require_checkout(
    args: argparse.Namespace, path: Union[Path, str, None]
) -> Tuple[EdenInstance, EdenCheckout, Path]:
    instance, checkout, rel_path = find_checkout(args, path)
    if checkout is None:
        msg_path = path if path is not None else os.getcwd()
        raise subcmd_mod.CmdError(f"no EdenFS checkout found at {msg_path}\n")
    assert rel_path is not None
    return instance, checkout, rel_path


def prompt_confirmation(prompt: str) -> bool:
    # Import readline lazily here because it conflicts with ncurses's resize support.
    # https://bugs.python.org/issue2675
    try:
        import readline  # noqa: F401 Importing readline improves the behavior of input()
    except ImportError:
        # We don't strictly need readline
        pass

    prompt_str = f"{prompt} [y/N] "
    while True:
        response = input(prompt_str)
        value = response.lower()
        if value in ("y", "yes"):
            return True
        if value in ("", "n", "no"):
            return False
        print('Please enter "yes" or "no"')
