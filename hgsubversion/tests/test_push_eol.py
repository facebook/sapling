import test_util

import unittest

class TestPushEol(test_util.TestBase):
    obsolete_mode_tests = True
    stupid_mode_tests = True

    def setUp(self):
        test_util.TestBase.setUp(self)
        self._load_fixture_and_fetch('emptyrepo.svndump')

    def test_push_dirs(self):
        changes = [
            # Root files with LF, CRLF and mixed EOL
            ('lf', 'lf', 'a\nb\n\nc'),
            ('crlf', 'crlf', 'a\r\nb\r\n\r\nc'),
            ('mixed', 'mixed', 'a\r\nb\n\r\nc\nd'),
            ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertchanges(changes, self.repo['tip'])

        changes = [
            # Update all files once, with same EOL
            ('lf', 'lf', 'a\nb\n\nc\na\nb\n\nc'),
            ('crlf', 'crlf', 'a\r\nb\r\n\r\nc\r\na\r\nb\r\n\r\nc'),
            ('mixed', 'mixed', 'a\r\nb\n\r\nc\nd\r\na\r\nb\n\r\nc\nd'),
            ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertchanges(changes, self.repo['tip'])
