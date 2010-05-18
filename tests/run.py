import optparse
import os
import sys
import unittest

def tests():
    import test_util
    import test_binaryfiles
    import test_diff
    import test_externals
    import test_fetch_branches
    import test_fetch_command
    import test_fetch_command_regexes
    import test_fetch_exec
    import test_fetch_mappings
    import test_fetch_renames
    import test_fetch_symlinks
    import test_fetch_truncated
    import test_pull
    import test_push_command
    import test_push_renames
    import test_push_dirs
    import test_push_eol
    import test_rebuildmeta
    import test_single_dir_clone
    import test_svnwrap
    import test_tags
    import test_utility_commands
    import test_urls

    sys.path.append(os.path.dirname(__file__))
    sys.path.append(os.path.join(os.path.dirname(__file__), 'comprehensive'))

    import test_stupid_pull
    import test_verify

    return locals()

def comprehensive(mod):
    dir = os.path.basename(os.path.dirname(mod.__file__))
    return dir == 'comprehensive'

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

    (options, args) = parser.parse_args()

    if options.verbose:
        testargs = { 'descriptions': 3, 'verbosity': 2 }
    else:
        testargs = {'descriptions': 2}

    # make sure our copy of hgsubversion gets imported
    sys.path.append(os.path.dirname(os.path.dirname(__file__)))

    if options.demandimport:
        from mercurial import demandimport
        demandimport.enable()

    # silence output when running outside nose
    sys.stdout = os.tmpfile()

    all = tests()
    del all['test_util']

    args = [i.split('.py')[0].replace('-', '_') for i in args]

    if not args:
        check = lambda x: options.comprehensive or not comprehensive(x)
        mods = [m for (n, m) in sorted(all.iteritems()) if check(m)]
        suite = [m.suite() for m in mods]
    else:
        suite = []
        for arg in args:
            if arg == 'test_util':
                continue
            elif arg not in all:
                print >> sys.stderr, 'test module %s not available' % arg
            else:
                suite.append(all[arg].suite())

    runner = unittest.TextTestRunner(**testargs)
    result = runner.run(unittest.TestSuite(suite))
    if not result.wasSuccessful():
        sys.exit(1)
