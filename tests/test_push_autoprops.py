import subprocess
import sys
import unittest
import os

import test_util

from hgsubversion import svnwrap

class PushAutoPropsTests(test_util.TestBase):
    obsolete_mode_tests = True

    def setUp(self):
        test_util.TestBase.setUp(self)
        repo, self.repo_path = self.load_and_fetch('emptyrepo.svndump')

    def test_push_honors_svn_autoprops(self):
        self.setup_svn_config(
            "[miscellany]\n"
            "enable-auto-props = yes\n"
            "[auto-props]\n"
            "*.py = test:prop=success\n")
        changes = [('test.py', 'test.py', 'echo hallo')]
        self.commitchanges(changes)
        self.pushrevisions(True)
        prop_val = test_util.svnpropget(
            self.repo_path, "trunk/test.py", 'test:prop')
        self.assertEqual('success', prop_val)


class AutoPropsConfigTest(test_util.TestBase):
    def test_use_autoprops_for_matching_file_when_enabled(self):
        self.setup_svn_config(
            "[miscellany]\n"
            "enable-auto-props = yes\n"
            "[auto-props]\n"
            "*.py = test:prop=success\n")
        props = self.new_autoprops_config().properties('xxx/test.py')
        self.assertEqual({ 'test:prop': 'success'}, props)

    def new_autoprops_config(self):
        return svnwrap.AutoPropsConfig(self.config_dir)

    def test_ignore_nonexisting_config(self):
        config_file = os.path.join(self.config_dir, 'config')
        os.remove(config_file)
        self.assertTrue(not os.path.exists(config_file))
        props = self.new_autoprops_config().properties('xxx/test.py')
        self.assertEqual({}, props)

    def test_ignore_autoprops_when_file_doesnt_match(self):
        self.setup_svn_config(
            "[miscellany]\n"
            "enable-auto-props = yes\n"
            "[auto-props]\n"
            "*.py = test:prop=success\n")
        props = self.new_autoprops_config().properties('xxx/test.sh')
        self.assertEqual({}, props)

    def test_ignore_autoprops_when_disabled(self):
        self.setup_svn_config(
            "[miscellany]\n"
            "#enable-auto-props = yes\n"
            "[auto-props]\n"
            "*.py = test:prop=success\n")
        props = self.new_autoprops_config().properties('xxx/test.py')
        self.assertEqual({}, props)

    def test_combine_properties_of_multiple_matches(self):
        self.setup_svn_config(
            "[miscellany]\n"
            "enable-auto-props = yes\n"
            "[auto-props]\n"
            "*.py = test:prop=success\n"
            "test.* = test:prop2=success\n")
        props = self.new_autoprops_config().properties('xxx/test.py')
        self.assertEqual({
            'test:prop': 'success', 'test:prop2': 'success'}, props)


class ParseAutoPropsTests(test_util.TestBase):
    def test_property_value_is_optional(self):
        props = svnwrap.parse_autoprops("svn:executable")
        self.assertEqual({'svn:executable': ''}, props)
        props = svnwrap.parse_autoprops("svn:executable=")
        self.assertEqual({'svn:executable': ''}, props)

    def test_property_value_may_be_quoted(self):
        props = svnwrap.parse_autoprops("svn:eol-style=\" native \"")
        self.assertEqual({'svn:eol-style': ' native '}, props)
        props = svnwrap.parse_autoprops("svn:eol-style=' native '")
        self.assertEqual({'svn:eol-style': ' native '}, props)

    def test_surrounding_whitespaces_are_ignored(self):
        props = svnwrap.parse_autoprops(" svn:eol-style = native ")
        self.assertEqual({'svn:eol-style': 'native'}, props)

    def test_multiple_properties_are_separated_by_semicolon(self):
        props = svnwrap.parse_autoprops(
            "svn:eol-style=native;svn:executable=true\n")
        self.assertEqual({
            'svn:eol-style': 'native',
            'svn:executable': 'true'},
            props)
