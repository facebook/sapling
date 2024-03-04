#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


import asyncio
import selectors

import IPython


def main() -> None:
    # Work around issue with ProactorEventLoop on Python 3.8.6:
    # https://github.com/prompt-toolkit/python-prompt-toolkit/issues/1023#issuecomment-563337948
    selector = selectors.SelectSelector()
    loop = asyncio.SelectorEventLoop(selector)
    asyncio.set_event_loop(loop)

    IPython.embed()


if __name__ == "__main__":
    main()
