import test_util

import unittest

class TestPushEol(test_util.TestBase):
    def setUp(self):
        test_util.TestBase.setUp(self)
        self._load_fixture_and_fetch('emptyrepo.svndump')

    def _test_push_dirs(self, stupid):
        changes = [
            # Root files with LF, CRLF and mixed EOL
            ('lf', 'lf', 'a\nb\n\nc'),
            ('crlf', 'crlf', 'a\r\nb\r\n\r\nc'),
            ('mixed', 'mixed', 'a\r\nb\n\r\nc\nd'),
            ]
        self.commitchanges(changes)
        self.pushrevisions(stupid)
        self.assertchanges(changes, self.repo['tip'])

        changes = [
            # Update all files once, with same EOL
            ('lf', 'lf', 'a\nb\n\nc\na\nb\n\nc'),
            ('crlf', 'crlf', 'a\r\nb\r\n\r\nc\r\na\r\nb\r\n\r\nc'),
            ('mixed', 'mixed', 'a\r\nb\n\r\nc\nd\r\na\r\nb\n\r\nc\nd'),
            ]
        self.commitchanges(changes)
        self.pushrevisions(stupid)
        self.assertchanges(changes, self.repo['tip'])

    def test_push_dirs(self):
        self._test_push_dirs(False)

    def test_push_dirs_stupid(self):
        self._test_push_dirs(True)
