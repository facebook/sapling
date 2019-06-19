#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import atexit
import os
import pathlib
import tempfile

import hypothesis.strategies as st
from eden.test_support.temporary_directory import cleanup_tmp_dir
from hypothesis import HealthCheck, settings
from hypothesis.configuration import hypothesis_home_dir, set_hypothesis_home_dir


def is_sandcastle() -> bool:
    return "SANDCASTLE" in os.environ


def fast_hypothesis_test():
    return settings(max_examples=1000, suppress_health_check=[])


def set_up_hypothesis() -> None:
    default_settings = settings(
        # Turn off the health checks because setUp/tearDown are too slow
        suppress_health_check=[HealthCheck.too_slow],
        # Turn off the example database; we don't have a way to persist this
        # or share this across runs, so we don't derive any benefit from it at
        # this time.
        database=None,
    )

    # Configure Hypothesis to run faster when iterating locally
    settings.register_profile(
        "dev", settings(default_settings, max_examples=5, timeout=0)
    )
    # ... and use the defaults (which have more combinations) when running
    # on CI, which we want to be more deterministic.
    settings.register_profile(
        "ci", settings(default_settings, derandomize=True, timeout=120)
    )

    # Use the dev profile by default, but use the ci profile on sandcastle.
    settings.load_profile(
        "ci" if is_sandcastle() else os.getenv("HYPOTHESIS_PROFILE", "dev")
    )

    # We need to set a global (but non-conflicting) path to store some state
    # during hypothesis example runs.  We want to avoid putting this state in
    # the repo.
    set_hypothesis_home_dir(tempfile.mkdtemp(prefix="eden_hypothesis."))
    atexit.register(cleanup_tmp_dir, pathlib.Path(hypothesis_home_dir()))


# Some helpers for Hypothesis decorators
FILENAME_STRATEGY = st.text(
    st.characters(min_codepoint=1, max_codepoint=1000, blacklist_characters="/:\\"),
    min_size=1,
)
