# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Builders shared by the reference and differential backend tests: the reference spec,
the fake sl, and the hang guard.
"""

from __future__ import annotations

from dataclasses import replace

from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Environment as SpecEnvironment,
    RunSpec,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _spec,
    REGISTERS,
)


REGISTER = REGISTERS[0]


SEED = 11


# A stand-in for Sapling over plain directories. A clone copies the source;
# the source's `.node` names its commit and `.origin` its own path, so status
# and diff report every tracked file whose bytes differ from the source.
FAKE_SL = r"""#!/bin/sh
here=$(pwd)
# Return once the background child names `sleep`, so the leader exits
# only after its straggler can be described by command line.
exec_seen() { until grep -q sleep "/proc/$1/cmdline" 2>/dev/null; do :; done; }
case "$1" in
clone) cp -r "$4" "$5" ;;
log) cat .node ;;
status)
  origin=$(cat .origin)
  cd "$origin" && find tree -type f | sort | while read -r f; do
    cmp -s "$f" "$here/$f" || printf 'M %s\0' "$f"
  done ;;
diff)
  origin=$(cat .origin)
  cd "$origin" && find tree -type f | sort | while read -r f; do
    cmp -s "$f" "$here/$f" || printf ' %s | 1 +-\n' "$f"
  done ;;
straggle) sleep 3600 >/dev/null 2>&1 & exec_seen $! ;;
straggle-pipe) sleep 3600 & exec_seen $! ;;
straggle-session) setsid sleep 3600 >/dev/null 2>&1 </dev/null & exec_seen $! ;;
umask) umask ;;
args) printf '%s\n' "$@" ;;
hang) exec sleep 3600 ;;
fail) echo refused >&2; exit 4 ;;
*) exit 3 ;;
esac
"""


# Real seconds before a test gives up waiting; a hang guard, not a measurement.
HANG_GUARD = 60.0


def reference_spec(
    actions: list[Action],
    actors: list[tuple[str, list[str]]] | None = None,
    *,
    reference: bool = True,
    case_sensitive: bool = True,
) -> RunSpec:
    spec = _spec(REGISTER, actions, actors or [("a", [a.id for a in actions])])
    target = dict(spec.environment.target)
    target["case_sensitive"] = case_sensitive
    target["setup_profile"] = {**target["setup_profile"], "reference_clone": reference}
    return replace(spec, environment=SpecEnvironment(target=target))
