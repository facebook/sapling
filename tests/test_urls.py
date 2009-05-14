import test_util
import unittest
from hgsubversion.svnwrap.svn_swig_wrapper import parse_url

class TestSubversionUrls(test_util.TestBase):
    def test_standard_url(self):
        self.assertEqual((None, None, 'file:///var/svn/repo'),
                         parse_url('file:///var/svn/repo'))

    def test_user_url(self):
        self.assertEqual(('joe', None, 'https://svn.testurl.com/repo'),
                         parse_url('https://joe@svn.testurl.com/repo'))

    def test_password_url(self):
        self.assertEqual((None, 't3stpw', 'svn+ssh://svn.testurl.com/repo'),
                         parse_url('svn+ssh://:t3stpw@svn.testurl.com/repo'))

    def test_user_password_url(self):
        self.assertEqual(('joe', 't3stpw', 'https://svn.testurl.com/repo'),
                         parse_url('https://joe:t3stpw@svn.testurl.com/repo'))


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestSubversionUrls)]
    return unittest.TestSuite(all)
