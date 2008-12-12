import urllib # for url quoting

from mercurial import ui
from mercurial import hg

import utility_commands

import test_util

expected_info_output = '''URL: file://%(repo)s/%(branch)s
Repository Root: None
Repository UUID: df2126f7-00ab-4d49-b42c-7e981dde0bcf
Revision: %(rev)s
Node Kind: directory
Last Changed Author: durin
Last Changed Rev: %(rev)s
Last Changed Date: %(date)s
'''

class UtilityTests(test_util.TestBase):
    def test_info_output(self):
        self._load_fixture_and_fetch('two_heads.svndump')
        hg.update(self.repo, 'the_branch')
        u = ui.ui()
        utility_commands.run_svn_info(u, self.repo, self.wc_path)
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:05 +0000 (Wed, 08 Oct 2008)',
                     'repo': urllib.quote(self.repo_path),
                     'branch': 'branches/the_branch',
                     'rev': 5,
                     })
        self.assertEqual(u.stream.getvalue(), expected)
        hg.update(self.repo, 'default')
        u = ui.ui()
        utility_commands.run_svn_info(u, self.repo, self.wc_path)
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:29 +0000 (Wed, 08 Oct 2008)',
                     'repo': urllib.quote(self.repo_path),
                     'branch': 'trunk',
                     'rev': 6,
                     })
        self.assertEqual(u.stream.getvalue(), expected)

    def test_url_output(self):
        self._load_fixture_and_fetch('two_revs.svndump')
        hg.update(self.repo, 'tip')
        u = ui.ui()
        utility_commands.print_wc_url(u, self.repo, self.wc_path)
        expected = 'file://%s\n' % urllib.quote(self.repo_path)
        self.assertEqual(u.stream.getvalue(), expected)
