# no-check-code -- see T24862348

from __future__ import absolute_import

import os
import sys

import test_hgsubversion_util


test_push_command = test_hgsubversion_util.import_test("test_push_command")


class ObsstoreOnMixIn(object):
    # do not double the test size by being wrapped again
    obsolete_mode_tests = False
    stupid_mode_tests = False

    def setUp(self):
        super(ObsstoreOnMixIn, self).setUp()
        hgrcpath = os.environ.get("HGRCPATH")
        assert hgrcpath
        with open(hgrcpath, "a") as f:
            f.write("\n[experimental]\nevolution=createmarkers\n")

    def shortDescription(self):
        text = super(ObsstoreOnMixIn, self).shortDescription()
        if text:
            text += " (obsstore on)"
        return text


def buildtestclass(cls):
    name = "ObsstoreOn%s" % cls.__name__
    newcls = type(name, (ObsstoreOnMixIn, cls), {})
    globals()[name] = newcls


buildtestclass(test_push_command.PushTests)

if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
