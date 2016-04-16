from __future__ import absolute_import
import os

if os.environ.get('COVERAGE_PROCESS_START'):
    try:
        import coverage
        import random

        # uuid is better, but not available in Python 2.4.
        covpath = os.path.join(os.environ['COVERAGE_DIR'],
                               'cov.%s' % random.randrange(0, 1000000000000))
        cov = coverage.coverage(data_file=covpath, auto_data=True)
        cov._warn_no_data = False
        cov._warn_unimported_source = False
        cov.start()
    except ImportError:
        pass
