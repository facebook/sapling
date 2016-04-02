# Helper module to use the Hypothesis tool in tests
#
# Copyright 2015 David R. MacIver
#
# For details see http://hypothesis.readthedocs.org

from __future__ import absolute_import, print_function
import os
import sys
import traceback

try:
    # hypothesis 2.x
    from hypothesis.configuration import set_hypothesis_home_dir
    from hypothesis import settings
except ImportError:
    # hypothesis 1.x
    from hypothesis.settings import set_hypothesis_home_dir
    from hypothesis import Settings as settings
import hypothesis.strategies as st
from hypothesis import given

# hypothesis store data regarding generate example and code
set_hypothesis_home_dir(os.path.join(
    os.getenv('TESTTMP'), ".hypothesis"
))

def check(*args, **kwargs):
    """decorator to make a function a hypothesis test

    Decorated function are run immediately (to be used doctest style)"""
    def accept(f):
        # Workaround for https://github.com/DRMacIver/hypothesis/issues/206
        # Fixed in version 1.13 (released 2015 october 29th)
        f.__module__ = '__anon__'
        try:
            with settings(max_examples=2000):
                given(*args, **kwargs)(f)()
        except Exception:
            traceback.print_exc(file=sys.stdout)
            sys.exit(1)
    return accept


def roundtrips(data, decode, encode):
    """helper to tests function that must do proper encode/decode roundtripping
    """
    @given(data)
    def testroundtrips(value):
        encoded = encode(value)
        decoded = decode(encoded)
        if decoded != value:
            raise ValueError(
                "Round trip failed: %s(%r) -> %s(%r) -> %r" % (
                    encode.__name__, value, decode.__name__, encoded,
                    decoded
                ))
    try:
        testroundtrips()
    except Exception:
        # heredoc swallow traceback, we work around it
        traceback.print_exc(file=sys.stdout)
        raise
    print("Round trip OK")


# strategy for generating bytestring that might be an issue for Mercurial
bytestrings = (
    st.builds(lambda s, e: s.encode(e), st.text(), st.sampled_from([
        'utf-8', 'utf-16',
    ]))) | st.binary()
