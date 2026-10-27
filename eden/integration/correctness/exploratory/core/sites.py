# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Target-neutral fault-site metadata."""

from __future__ import annotations

import random
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from typing import Any


SiteValidator = Callable[[Any, Any, Any], None]

PUBLIC_MASTER = "public-master"
DAEMON_LABELS = (PUBLIC_MASTER, "stack-build")
# Where a site sits relative to its target's effect: an error injected at a
# `pre` site fails the target before the effect, one at a `post` site after it.
PRE_EFFECT = "pre"
POST_EFFECT = "post"
EFFECT_POSITIONS = (PRE_EFFECT, POST_EFFECT)


@dataclass(frozen=True)
class EngagePlan:
    """Least engagement that lets one site's target reach its checkpoint.

    ``pre_install_actions`` are (kind, payload) pairs the composer numbers
    e0, e1, ... and finishes before the install. ``target_payload`` merges
    into the sampled target; a None value drops that key.
    """

    setup_profile_overrides: Mapping[str, Any]
    pre_install_actions: tuple[tuple[str, Mapping[str, Any]], ...]
    target_payload: Mapping[str, Any]


Engage = Callable[[random.Random, Mapping[str, Any]], EngagePlan]


@dataclass(frozen=True)
class Site:
    """One target checkpoint that a fault action may arm.

    `effect` declares whether the checkpoint runs before (`pre`) or after
    (`post`) the target's effect has taken place.
    """

    key_class: str
    target_kinds: frozenset[str]
    behaviors: frozenset[str]
    daemon: str
    validate: SiteValidator
    effect: str
    fixed_key: str | None = None
    name_keyed: bool = False
    witness_any: bool = False
    engage: Engage | None = None

    def __post_init__(self) -> None:
        """Reject metadata that cannot select a supported daemon fault."""
        if not self.behaviors:
            raise ValueError("site behaviors must be non-empty")
        if self.daemon not in DAEMON_LABELS:
            raise ValueError(f"unsupported site daemon {self.daemon!r}")
        if self.effect not in EFFECT_POSITIONS:
            raise ValueError(f"unsupported site effect position {self.effect!r}")
