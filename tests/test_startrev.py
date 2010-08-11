import test_util

import os
import unittest

def _do_case(self, name, subdir, stupid):
    wc_base = self.wc_path
    self.wc_path = wc_base + '_full'
    headclone = self._load_fixture_and_fetch(name, subdir=subdir, stupid=stupid,
                                             layout='single', startrev='HEAD')
    self.wc_path = wc_base + '_head'
    fullclone = self._load_fixture_and_fetch(name, subdir=subdir, stupid=stupid,
                                             layout='single')

    fulltip = fullclone['tip']
    headtip = headclone['tip']
    # viewing diff's of lists of files is easier on the eyes
    self.assertMultiLineEqual('\n'.join(fulltip), '\n'.join(headtip))

    for f in fulltip:
        self.assertMultiLineEqual(fulltip[f].data(), headtip[f].data())

def buildmethod(case, name, subdir, stupid):
    m = lambda self: self._do_case(case, subdir.strip('/'), stupid)
    m.__name__ = name
    m.__doc__ = ('Test clone with startrev on %s%s with %s replay.' %
                 (case, subdir, (stupid and 'stupid') or 'real'))
    return m


attrs = {'_do_case': _do_case,
         }
for case in [f for f in os.listdir(test_util.FIXTURES) if f.endswith('.svndump')]:
    subdir = test_util.subdir.get(case, '') + '/trunk'

    bname = 'test_' + case[:-len('.svndump')]
    attrs[bname] = buildmethod(case, bname, subdir, False)
    name = bname + '_stupid'
    attrs[name] = buildmethod(case, name, subdir, True)

StartRevTests = type('StartRevTests', (test_util.TestBase, ), attrs)


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(StartRevTests),
          ]
    return unittest.TestSuite(all)
