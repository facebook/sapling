import os
import unittest
import sys

# wrapped in a try/except because of weirdness in how
# run.py works as compared to nose.
try:
    import test_util
except ImportError:
    sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))
    import test_util

# interesting and fast tests
import test_fetch_mappings
import test_fetch_renames
import test_pull
import test_template_keywords
import test_utility_commands

# comprehensive tests
try:
    import test_custom_layout
except ImportError:
    sys.path.insert(0, os.path.dirname(__file__))
    import test_custom_layout

import test_rebuildmeta
import test_updatemeta

from hgsubversion import svnmeta, maps


class SqliteRevMapMixIn(object):
    # do not double the test size by being wrapped again
    obsolete_mode_tests = False
    stupid_mode_tests = False

    def setUp(self):
        assert svnmeta.SVNMeta._defaultrevmapclass is maps.RevMap
        svnmeta.SVNMeta._defaultrevmapclass = maps.SqliteRevMap
        super(SqliteRevMapMixIn, self).setUp()

    def tearDown(self):
        assert svnmeta.SVNMeta._defaultrevmapclass is maps.SqliteRevMap
        svnmeta.SVNMeta._defaultrevmapclass = maps.RevMap
        super(SqliteRevMapMixIn, self).tearDown()

    def shortDescription(self):
        text = super(SqliteRevMapMixIn, self).shortDescription()
        if text:
            text += ' (sqlite revmap)'
        return text

def buildtestclass(cls, selector=None):
    name = 'SqliteRevMap%s' % cls.__name__
    newcls = type(name, (SqliteRevMapMixIn, cls,), {})

    # remove test cases not selected by selector
    if selector:
        for name in dir(newcls):
            if name.startswith('test_') and not selector(name[5:]):
                setattr(newcls, name, None)

    globals()[name] = newcls

def svndumpselector(name):
    return name in ['branch_rename_to_trunk',
                    'tag_name_same_as_branch']

buildtestclass(test_fetch_mappings.MapTests)
buildtestclass(test_fetch_renames.TestFetchRenames)
buildtestclass(test_pull.TestPull)
buildtestclass(test_template_keywords.TestLogKeywords)
buildtestclass(test_utility_commands.UtilityTests)

buildtestclass(test_rebuildmeta.RebuildMetaTests, svndumpselector)
buildtestclass(test_updatemeta.UpdateMetaTests, svndumpselector)
