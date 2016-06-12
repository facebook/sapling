#!/usr/bin/env python

import optparse
import os
import sys

if sys.version_info[:2] < (2, 7):
    import unittest2 as unittest
else:
    import unittest

if __name__ == '__main__':
    description = ("This script runs the hgsubversion tests. If no tests are "
                   "specified, all known tests are implied.")
    parser = optparse.OptionParser(usage="%prog [options] [TESTS ...]",
                                   description=description)
    parser.add_option("-A", "--all",
                      dest="comprehensive", action="store_true", default=False,
                      help="include slow, but comprehensive tests")
    parser.add_option("-v", "--verbose",
                      dest="verbose", action="store_true", default=False,
                      help="enable verbose output")
    parser.add_option("", "--no-demandimport",
                      dest="demandimport", action="store_false", default=True,
                      help="disable Mercurial demandimport loading")
    parser.add_option("", "--bindings",
                      dest="bindings", action="store", default=None,
                      choices=["swig", "subvertpy"],
                      help="test using the specified bindings (swig or "
                      "subvertpy)")
    parser.add_option("", "--show-stdout",
                      dest="showstdout", action="store_true", default=False,
                      help="show stdout (hidden by default)")

    (options, args) = parser.parse_args()

    if options.verbose:
        testargs = { 'descriptions': 3, 'verbosity': 2 }
    else:
        testargs = {'descriptions': 2}

    sys.path.append(os.path.dirname(os.path.dirname(__file__)))

    if options.demandimport:
        from mercurial import demandimport
        demandimport.enable()

    if options.bindings:
        os.environ['HGSUBVERSION_BINDINGS'] = options.bindings

    # make sure our copy of hgsubversion gets imported by loading test_util
    import test_util
    test_util.TestBase

    # silence output when running outside nose
    if not options.showstdout:
        import tempfile
        sys.stdout = tempfile.TemporaryFile()

    args = [os.path.basename(os.path.splitext(arg)[0]).replace('-', '_')
            for arg in args]

    loader = unittest.TestLoader()
    suite = unittest.TestSuite()

    if not args:
        suite = loader.discover('.')

        if options.comprehensive:
            suite.addTests(loader.discover('comprehensive',
                                           top_level_dir='comprehensive'))
    else:
        sys.path.append(os.path.join(os.path.dirname(__file__), 'comprehensive'))

        suite.addTests(loader.loadTestsFromNames(args))

    runner = unittest.TextTestRunner(**testargs)
    result = runner.run(suite)
    if not result.wasSuccessful():
        sys.exit(1)
