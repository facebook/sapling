#!/usr/bin/env python

import optparse
import os
import subprocess
import sys

# PassThroughOptionParse is from the Optik source distribution, (c) 2001-2006
# Gregory P. Ward. Used under the BSD license.
class PassThroughOptionParser(optparse.OptionParser):
    def _process_long_opt(self, rargs, values):
        try:
            optparse.OptionParser._process_long_opt(self, rargs, values)
        except optparse.BadOptionError as err:
            self.largs.append(err.opt_str)

    def _process_short_opts(self, rargs, values):
        try:
            optparse.OptionParser._process_short_opts(self, rargs, values)
        except optparse.BadOptionError as err:
            self.largs.append(err.opt_str)

def parseargs(argv):
    parser = PassThroughOptionParser(usage='%prog [options]',
        epilog='Any additional options and arguments are passed through to '
               'REPO/tests/run-tests.py.')

    parser.add_option('--hg', type='string',
        metavar='REPO',
        help='Mercurial repository to run tests against')
    parser.add_option('--disable-blacklist', action='store_true',
        default=False,
        help='disable default test blacklist')

    options, args = parser.parse_args(argv)
    if not options.hg:
        parser.error('Mercurial repository not specified')

    return options, args

def main(argv):
    options, args = parseargs(argv)

    thisdir = os.path.dirname(os.path.realpath(__file__))
    extroot = os.path.join(os.path.dirname(thisdir), 'sqldirstate')
    extopts = ['--extra-config-opt', 'extensions.sqldirstate=%s' % extroot,
               '--extra-config-opt', 'sqldirstate.skipbackups=False',
               '--extra-config-opt', 'format.sqldirstate=True']
    if not options.disable_blacklist:
        extopts += ['--blacklist',
                    os.path.join(thisdir, 'blacklist-sqldirstate')]

    cwd = os.path.expanduser(os.path.join(options.hg, 'tests'))
    cmd = [os.path.join(cwd, 'run-tests.py')] + extopts + args

    return subprocess.call(cmd, cwd=cwd)

if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
