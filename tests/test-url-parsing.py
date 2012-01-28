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
        # also test that it works even if heroku isn't in the name
        url = "git+ssh://git@compatible.com:webjam.git"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, 'webjam.git')
        self.assertEquals(client.host, 'git@compatible.com')

    def test_ssh_heroku_style_with_trailing_slash(self):
        # some versions of mercurial add a trailing slash even if
        #  the user didn't supply one.
        url = "git+ssh://git@heroku.com:webjam.git/"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, 'webjam.git')
        self.assertEquals(client.host, 'git@heroku.com')

    def test_heroku_style_with_port(self):
        url = "git+ssh://git@heroku.com:999:webjam.git"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, 'webjam.git')
        self.assertEquals(client.host, 'git@heroku.com')
        self.assertEquals(client.port, '999')

    def test_gitdaemon_style(self):
        url = "git://github.com/webjam/webjam.git"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, '/webjam/webjam.git')
        try:
            self.assertEquals(client._host, 'github.com')
        except AttributeError:
            self.assertEquals(client.host, 'github.com')

    def test_ssh_github_style_slash_with_port(self):
        url = "git+ssh://git@github.com:10022/webjam/webjam.git"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, '/webjam/webjam.git')
        self.assertEquals(client.host, 'git@github.com')
        self.assertEquals(client.port, '10022')

    def test_gitdaemon_style_with_port(self):
        url = "git://github.com:19418/webjam/webjam.git"
        client, path = self.handler.get_transport_and_path(url)
        self.assertEquals(path, '/webjam/webjam.git')
        try:
            self.assertEquals(client._host, 'github.com')
        except AttributeError:
            self.assertEquals(client.host, 'github.com')
        self.assertEquals(client._port, '19418')

if __name__ == '__main__':
    tc = TestUrlParsing()
    for test in ['test_ssh_github_style_slash',
                 'test_ssh_github_style_colon',
                 'test_ssh_heroku_style',
                 'test_ssh_heroku_style_with_trailing_slash',
                 'test_heroku_style_with_port',
                 'test_gitdaemon_style',
                 'test_ssh_github_style_slash_with_port',
                 'test_gitdaemon_style_with_port']:
        tc.setUp()
        getattr(tc, test)()
        tc.tearDown()
