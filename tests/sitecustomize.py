from __future__ import absolute_import
import os

if os.environ.get('COVERAGE_PROCESS_START'):
    try:
        import coverage
        import uuid

        covpath = os.path.join(os.environ['COVERAGE_DIR'],
                               'cov.%s' % uuid.uuid1())
        cov = coverage.coverage(data_file=covpath, auto_data=True)
        cov._warn_no_data = False
        cov._warn_unimported_source = False
        cov.start()
    except ImportError:
        pass
