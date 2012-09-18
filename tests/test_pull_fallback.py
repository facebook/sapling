import test_util

import re
import mercurial
from mercurial import commands
from hgsubversion import stupid
from hgsubversion import svnwrap
from hgsubversion import wrappers

class TestPullFallback(test_util.TestBase):
    def setUp(self):
        super(TestPullFallback, self).setUp()

    def _loadupdate(self, fixture_name, *args, **kwargs):
        kwargs = kwargs.copy()
        kwargs.update(noupdate=False)
        repo, repo_path = self.load_and_fetch(fixture_name, *args, **kwargs)
        return repo, repo_path

    def test_stupid_fallback_to_stupid_fullrevs(self):
        return
        to_patch = {
            'mercurial.patch.patchbackend': _patchbackend_raise,
            'stupid.diff_branchrev': stupid.diff_branchrev,
            'stupid.fetch_branchrev': stupid.fetch_branchrev,
        }

        expected_calls = {
            'mercurial.patch.patchbackend': 1,
            'stupid.diff_branchrev': 1,
            'stupid.fetch_branchrev': 1,
        }

        repo, repo_path = self._loadupdate(
            'single_rev.svndump', stupid=True)

        # Passing stupid=True doesn't seem to be working - force it
        repo.ui.setconfig('hgsubversion', 'stupid', "true")
        state = repo.parents()

        calls, replaced = _monkey_patch(to_patch)

        try:
            self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed'})
            commands.pull(self.repo.ui, repo, update=True)
            self.failIfEqual(state, repo.parents())
            self.assertTrue('tip' in repo[None].tags())
            self.assertEqual(expected_calls, calls)

        finally:
            _monkey_unpatch(replaced)

def _monkey_patch(to_patch, start=None):
    if start is None:
        import sys
        start = sys.modules[__name__]

    calls = {}
    replaced = {}

    for path, replacement in to_patch.iteritems():
        obj = start
        owner, attr = path.rsplit('.', 1)

        for a in owner.split('.', -1):
            obj = getattr(obj, a)

        replaced[path] = getattr(obj, attr)
        calls[path] = 0

        def outer(path=path, calls=calls, replacement=replacement):
            def wrapper(*p, **kw):
                calls[path] += 1
                return replacement(*p, **kw)

            return wrapper

        setattr(obj, attr, outer())

    return calls, replaced

def _monkey_unpatch(to_patch, start=None):
    if start is None:
        import sys
        start = sys.modules[__name__]

    replaced = {}

    for path, replacement in to_patch.iteritems():
        obj = start
        owner, attr = path.rsplit('.', 1)

        for a in owner.split('.', -1):
            obj = getattr(obj, a)

        replaced[path] = getattr(obj, attr)
        setattr(obj, attr, replacement)

    return replaced

def _patchbackend_raise(*p, **kw):
    raise mercurial.patch.PatchError("patch failed")

def suite():
    import unittest, sys
    return unittest.findTestCases(sys.modules[__name__])
