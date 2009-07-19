import test_util
import unittest
from hgsubversion.svnwrap.svn_swig_wrapper import parse_url

class TestSubversionUrls(test_util.TestBase):
    def test_standard_url(self):
        self.assertEqual((None, None, 'file:///var/svn/repo'),
                         parse_url('file:///var/svn/repo'))

    def test_user_url(self):
        self.assertEqual(
            ('joe', None, 'https://svn.testurl.com/repo'),
            parse_url('https://joe@svn.testurl.com/repo'))
        self.assertEqual(
            ('bob', None, 'https://svn.testurl.com/repo'),
            parse_url('https://joe@svn.testurl.com/repo', 'bob'))

    def test_password_url(self):
        self.assertEqual(
            (None, 't3stpw', 'svn+ssh://svn.testurl.com/repo'),
            parse_url('svn+ssh://:t3stpw@svn.testurl.com/repo'))
        self.assertEqual(
            (None, '123abc', 'svn+ssh://svn.testurl.com/repo'),
            parse_url('svn+ssh://:t3stpw@svn.testurl.com/repo', None, '123abc'))

    def test_svnssh_preserve_user(self):
        self.assertEqual(
            ('user', 't3stpw', 'svn+ssh://user@svn.testurl.com/repo', ),
            parse_url('svn+ssh://user:t3stpw@svn.testurl.com/repo'))
        self.assertEqual(
            ('bob', '123abc', 'svn+ssh://bob@svn.testurl.com/repo', ),
            parse_url('svn+ssh://user:t3stpw@svn.testurl.com/repo', 'bob', '123abc'))
        self.assertEqual(
            ('user2', None, 'svn+ssh://user2@svn.testurl.com/repo', ),
            parse_url('svn+ssh://user2@svn.testurl.com/repo'))
        self.assertEqual(
            ('bob', None, 'svn+ssh://bob@svn.testurl.com/repo', ),
            parse_url('svn+ssh://user2@svn.testurl.com/repo', 'bob'))

    def test_user_password_url(self):
        self.assertEqual(
            ('joe', 't3stpw', 'https://svn.testurl.com/repo'),
            parse_url('https://joe:t3stpw@svn.testurl.com/repo'))
        self.assertEqual(
            ('bob', '123abc', 'https://svn.testurl.com/repo'),
            parse_url('https://joe:t3stpw@svn.testurl.com/repo', 'bob', '123abc'))


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestSubversionUrls)]
    return unittest.TestSuite(all)
