import os
import sys

# wrapped in a try/except because of weirdness in how
# run.py works as compared to nose.
try:
    import test_util
except ImportError:
    sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))
    import test_util

import test_push_command


class ObsstoreOnMixIn(object):
    # do not double the test size by being wrapped again
    obsolete_mode_tests = False
    stupid_mode_tests = False

    def setUp(self):
        super(ObsstoreOnMixIn, self).setUp()
        hgrcpath = os.environ.get('HGRCPATH')
        assert hgrcpath
        with open(hgrcpath, 'a') as f:
            f.write('\n[experimental]\nevolution=createmarkers\n')

    def shortDescription(self):
        text = super(ObsstoreOnMixIn, self).shortDescription()
        if text:
            text += ' (obsstore on)'
        return text


def buildtestclass(cls):
    name = 'ObsstoreOn%s' % cls.__name__
    newcls = type(name, (ObsstoreOnMixIn, cls,), {})
    globals()[name] = newcls


buildtestclass(test_push_command.PushTests)
