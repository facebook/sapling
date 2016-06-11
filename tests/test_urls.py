import test_util

import unittest
import urllib

from hgsubversion.svnwrap import parse_url
from hgsubversion import svnrepo

class TestSubversionUrls(test_util.TestBase):
    def test_standard_url(self):
        self.check_parse_url((None, None, 'file:///var/svn/repo'),
                             ('file:///var/svn/repo', ))

    def test_user_url(self):
        self.check_parse_url(
            ('joe', None, 'https://svn.testurl.com/repo'),
            ('https://joe@svn.testurl.com/repo', ))
        self.check_parse_url(
            ('bob', None, 'https://svn.testurl.com/repo'),
            ('https://joe@svn.testurl.com/repo', 'bob', ))

    def test_password_url(self):
        self.check_parse_url(
            (None, 't3stpw', 'svn+ssh://svn.testurl.com/repo'),
            ('svn+ssh://:t3stpw@svn.testurl.com/repo', ))
        self.check_parse_url(
            (None, '123abc', 'svn+ssh://svn.testurl.com/repo'),
            ('svn+ssh://:t3stpw@svn.testurl.com/repo', None, '123abc', ))

    def test_svnssh_preserve_user(self):
        self.check_parse_url(
            ('user', 't3stpw', 'svn+ssh://user@svn.testurl.com/repo',),
            ('svn+ssh://user:t3stpw@svn.testurl.com/repo', ))
        self.check_parse_url(
            ('bob', '123abc', 'svn+ssh://bob@svn.testurl.com/repo',),
            ('svn+ssh://user:t3stpw@svn.testurl.com/repo', 'bob', '123abc', ))
        self.check_parse_url(
            ('user2', None, 'svn+ssh://user2@svn.testurl.com/repo',),
            ('svn+ssh://user2@svn.testurl.com/repo', ))
        self.check_parse_url(
            ('bob', None, 'svn+ssh://bob@svn.testurl.com/repo',),
            ('svn+ssh://user2@svn.testurl.com/repo', 'bob', ))

    def test_user_password_url(self):
        self.check_parse_url(
            ('joe', 't3stpw', 'https://svn.testurl.com/repo'),
            ('https://joe:t3stpw@svn.testurl.com/repo', ))
        self.check_parse_url(
            ('bob', '123abc', 'https://svn.testurl.com/repo'),
            ('https://joe:t3stpw@svn.testurl.com/repo', 'bob', '123abc', ))

    def test_url_rewriting(self):
        ui = test_util.ui.ui()
        ui.setconfig('hgsubversion', 'username', 'bob')
        repo = svnrepo.svnremoterepo(ui, 'svn+ssh://joe@foo/bar')
        self.assertEqual('svn+ssh://bob@foo/bar', repo.svnauth[0])
        self.assertEqual('svn+ssh://bob@foo/bar', repo.svnurl)

        repo = svnrepo.svnremoterepo(ui, 'svn+http://joe@foo/bar')
        self.assertEqual(('http://foo/bar', 'bob', None), repo.svnauth)
        self.assertEqual('http://foo/bar', repo.svnurl)

        repo = svnrepo.svnremoterepo(ui, 'svn+https://joe@foo/bar')
        self.assertEqual(('https://foo/bar', 'bob', None), repo.svnauth)
        self.assertEqual('https://foo/bar', repo.svnurl)

    def test_quoting(self):
        ui = self.ui()
        repo_path = self.load_svndump('non_ascii_path_1.svndump')

        repo_url = test_util.fileurl(repo_path)
        subdir = '/b\xC3\xB8b'
        quoted_subdir = urllib.quote(subdir)

        repo1 = svnrepo.svnremoterepo(ui, repo_url + subdir)
        repo2 = svnrepo.svnremoterepo(ui, repo_url + quoted_subdir)
        self.assertEqual(repo1.svnurl, repo2.svnurl)

    def check_parse_url(self, expected, args):
        self.assertEqual(expected, parse_url(*args))
        if len(args) == 1:
            repo = svnrepo.svnremoterepo(self.ui(), path=args[0])
            self.assertEqual(expected[2], repo.svnauth[0])
            self.assertEqual(expected[2], repo.svnurl)

