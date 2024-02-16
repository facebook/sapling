#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


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
