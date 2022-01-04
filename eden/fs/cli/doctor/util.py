# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from pathlib import Path
from typing import Optional

from eden.fs.cli.config import EdenCheckout, EdenInstance
from facebook.eden.ttypes import MountState


class CheckoutInfo:
    def __init__(
        self,
        instance: EdenInstance,
        path: Path,
        running_state_dir: Optional[Path] = None,
        configured_state_dir: Optional[Path] = None,
        state: Optional[MountState] = None,
    ) -> None:
        self.instance = instance
        self.path = path
        self.running_state_dir = running_state_dir
        self.configured_state_dir = configured_state_dir
        self.state = state

    def get_checkout(self) -> EdenCheckout:
        state_dir = (
            self.running_state_dir
            if self.running_state_dir is not None
            else self.configured_state_dir
        )
        assert state_dir is not None
        return EdenCheckout(self.instance, self.path, state_dir)
