import test_util

from mercurial import util as hgutil
from hgsubversion import svnmeta, maps
from mercurial.node import hex

class TestRevMapMigrate(test_util.TestBase):

    def _test_revmap_migrate(self, fromclass, toclass):
        # revmap interfaces to test
        getters = [
            lambda x: x.branchedits('the_branch', 3),
            lambda x: x.branchedits('the_branch', 4),
            lambda x: x.branchedits('the_branch', 5),
            lambda x: x.branchedits('the_branch', 6),
            lambda x: x.branchedits(None, 5),
            lambda x: x.branchedits('non_existed', 10),
            lambda x: x.branchmaxrevnum('the_branch', 3),
            lambda x: x.branchmaxrevnum('the_branch', 4),
            lambda x: x.branchmaxrevnum('the_branch', 5),
            lambda x: x.branchmaxrevnum('the_branch', 6),
            lambda x: x.branchmaxrevnum(None, 5),
            lambda x: x.branchmaxrevnum('non_existed', 10),
            lambda x: list(x.revhashes(3)),
            lambda x: list(x.revhashes(4)),
            lambda x: list(x.revhashes(42)),
            lambda x: list(x.revhashes(105)),
            lambda x: x.firstpulled,
            lambda x: x.lastpulled,
            lambda x: x.lasthash,
        ]

        svnmeta.SVNMeta._defaultrevmapclass = fromclass
        repo = self._load_fixture_and_fetch('two_heads.svndump')
        meta = svnmeta.SVNMeta(repo)
        self.assertEqual(meta.revmap.__class__, fromclass)
        origrevmap = meta.revmap

        # insert fake special (duplicated, with '\0') data
        origrevmap[103, None] = b'\0' * 20
        origrevmap[104, None] = b'\0' * 18 + b'cd'
        origrevmap[105, None] = b'ab\0cdefghijklmnopqrs'
        origrevmap[104, None] = b'\0' * 18 + b'\xff\0'
        origrevmap[105, 'ab'] = origrevmap[105, None]

        origvalues = [f(meta.revmap) for f in getters]

        # migrate to another format (transparently)
        svnmeta.SVNMeta._defaultrevmapclass = toclass
        meta = svnmeta.SVNMeta(repo)
        self.assertEqual(meta.revmap.__class__, toclass)

        # enable iteration otherwise we cannot use iteritems
        origrevmap._allowiter = True
        for k, v in origrevmap.iteritems():
            newv = meta.revmap[k]
            self.assertEqual(newv, v)
            self.assertEqual(len(newv), 20)
            self.assertEqual(meta.revmap[meta.revmap.hashes()[v]], v)

        newvalues = [f(meta.revmap) for f in getters]
        self.assertEqual(origvalues, newvalues)

    def test_revmap_migrate_up(self):
        self._test_revmap_migrate(maps.RevMap, maps.SqliteRevMap)

    def test_revmap_migrate_down(self):
        self._test_revmap_migrate(maps.SqliteRevMap, maps.RevMap)
