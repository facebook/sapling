# no-check-code -- see T24862348

import os
import sys

import test_hgsubversion_util
from hgext.hgsubversion import maps, svnmeta


# interesting and fast tests
test_fetch_mappings = test_hgsubversion_util.import_test("test_fetch_mappings")
test_fetch_renames = test_hgsubversion_util.import_test("test_fetch_renames")
test_pull = test_hgsubversion_util.import_test("test_pull")
test_template_keywords = test_hgsubversion_util.import_test("test_template_keywords")
test_utility_commands = test_hgsubversion_util.import_test("test_utility_commands")
test_custom_layout = test_hgsubversion_util.import_test("test_custom_layout")
test_rebuildmeta = test_hgsubversion_util.import_test("test_rebuildmeta")
test_updatemeta = test_hgsubversion_util.import_test("test_updatemeta")


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
            text += " (sqlite revmap)"
        return text


def buildtestclass(cls, selector=None):
    name = "SqliteRevMap%s" % cls.__name__
    newcls = type(name, (SqliteRevMapMixIn, cls), {})

    # remove test cases not selected by selector
    if selector:
        for name in dir(newcls):
            if name.startswith("test_") and not selector(name[5:]):
                setattr(newcls, name, None)

    globals()[name] = newcls


def svndumpselector(name):
    return name in ["branch_rename_to_trunk", "tag_name_same_as_branch"]


buildtestclass(test_fetch_mappings.MapTests)
buildtestclass(test_fetch_renames.TestFetchRenames)
buildtestclass(test_pull.TestPull)
buildtestclass(test_template_keywords.TestLogKeywords)
buildtestclass(test_utility_commands.UtilityTests)

buildtestclass(test_rebuildmeta.RebuildMetaTests, svndumpselector)
buildtestclass(test_updatemeta.UpdateMetaTests, svndumpselector)

if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
