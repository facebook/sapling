import io
import os

try:
    import unittest2 as unittest
except ImportError:
    import unittest

try:
    import hypothesis
    import hypothesis.strategies as strategies
except ImportError:
    raise unittest.SkipTest('hypothesis not available')

import zstd

from .common import (
    make_cffi,
)


s_windowlog = strategies.integers(min_value=zstd.WINDOWLOG_MIN,
                                    max_value=zstd.WINDOWLOG_MAX)
s_chainlog = strategies.integers(min_value=zstd.CHAINLOG_MIN,
                                    max_value=zstd.CHAINLOG_MAX)
s_hashlog = strategies.integers(min_value=zstd.HASHLOG_MIN,
                                max_value=zstd.HASHLOG_MAX)
s_searchlog = strategies.integers(min_value=zstd.SEARCHLOG_MIN,
                                    max_value=zstd.SEARCHLOG_MAX)
s_searchlength = strategies.integers(min_value=zstd.SEARCHLENGTH_MIN,
                                        max_value=zstd.SEARCHLENGTH_MAX)
s_targetlength = strategies.integers(min_value=zstd.TARGETLENGTH_MIN,
                                        max_value=zstd.TARGETLENGTH_MAX)
s_strategy = strategies.sampled_from((zstd.STRATEGY_FAST,
                                        zstd.STRATEGY_DFAST,
                                        zstd.STRATEGY_GREEDY,
                                        zstd.STRATEGY_LAZY,
                                        zstd.STRATEGY_LAZY2,
                                        zstd.STRATEGY_BTLAZY2,
                                        zstd.STRATEGY_BTOPT))


@make_cffi
@unittest.skipUnless('ZSTD_SLOW_TESTS' in os.environ, 'ZSTD_SLOW_TESTS not set')
class TestCompressionParametersHypothesis(unittest.TestCase):
    @hypothesis.given(s_windowlog, s_chainlog, s_hashlog, s_searchlog,
                        s_searchlength, s_targetlength, s_strategy)
    def test_valid_init(self, windowlog, chainlog, hashlog, searchlog,
                        searchlength, targetlength, strategy):
        # ZSTD_checkCParams moves the goal posts on us from what's advertised
        # in the constants. So move along with them.
        if searchlength == zstd.SEARCHLENGTH_MIN and strategy in (zstd.STRATEGY_FAST, zstd.STRATEGY_GREEDY):
            searchlength += 1
        elif searchlength == zstd.SEARCHLENGTH_MAX and strategy != zstd.STRATEGY_FAST:
            searchlength -= 1

        p = zstd.CompressionParameters(windowlog, chainlog, hashlog,
                                        searchlog, searchlength,
                                        targetlength, strategy)

        cctx = zstd.ZstdCompressor(compression_params=p)
        with cctx.write_to(io.BytesIO()):
            pass

    @hypothesis.given(s_windowlog, s_chainlog, s_hashlog, s_searchlog,
                        s_searchlength, s_targetlength, s_strategy)
    def test_estimate_compression_context_size(self, windowlog, chainlog,
                                                hashlog, searchlog,
                                                searchlength, targetlength,
                                                strategy):
        if searchlength == zstd.SEARCHLENGTH_MIN and strategy in (zstd.STRATEGY_FAST, zstd.STRATEGY_GREEDY):
            searchlength += 1
        elif searchlength == zstd.SEARCHLENGTH_MAX and strategy != zstd.STRATEGY_FAST:
            searchlength -= 1

        p = zstd.CompressionParameters(windowlog, chainlog, hashlog,
                            searchlog, searchlength,
                            targetlength, strategy)
        size = zstd.estimate_compression_context_size(p)
