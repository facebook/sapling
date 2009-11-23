import os, sys, tempfile, unittest, shutil
from mercurial import ui, hg, commands

sys.path.append(os.path.join(os.path.dirname(__file__), os.path.pardir))

from hggit.git_handler import GitHandler


class TestUrlParsing(object):
    def setUp(self):
        # create a test repo location.
        self.tmpdir = tempfile.mkdtemp('hg-git_url-test')
        commands.init(ui.ui(), self.tmpdir)
        repo = hg.repository(ui.ui(), self.tmpdir)
        self.handler = GitHandler(repo, ui.ui())

    def tearDown(self):
        # remove the temp repo
        shutil.rmtree(self.tmpdir)

    def assertEquals(self, l, r):
        print '%% expect %r' % (r, )
        print l
        assert l == r

    def test_ssh_github_style_slash(self):
        url = "git+ssh://git@github.com/webjam/webjam.git"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, '/webjam/webjam.git')
        self.assertEquals(client.host, 'git@github.com')

    def test_ssh_github_style_colon(self):
        url = "git+ssh://git@github.com:webjam/webjam.git"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, 'webjam/webjam.git')
        self.assertEquals(client.host, 'git@github.com')

    def test_ssh_heroku_style(self):
        url = "git+ssh://git@heroku.com:webjam.git"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, 'webjam.git')
        self.assertEquals(client.host, 'git@heroku.com')

    def test_gitdaemon_style(self):
        url = "git://github.com/webjam/webjam.git"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, '/webjam/webjam.git')
        self.assertEquals(client.host, 'github.com')


if __name__ == '__main__':
    tc = TestUrlParsing()
    for test in ['test_ssh_github_style_slash',
                 'test_ssh_github_style_colon',
                 'test_ssh_heroku_style',
                 'test_gitdaemon_style']:
        tc.setUp()
        getattr(tc, test)()
        tc.tearDown()
