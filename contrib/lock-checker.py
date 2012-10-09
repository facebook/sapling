"""Extension to verify locks are obtained in the required places.

This works by wrapping functions that should be surrounded by a lock
and asserting the lock is held. Missing locks are called out with a
traceback printed to stderr.

This currently only checks store locks, not working copy locks.
"""
import os
import traceback

def _warnstack(ui, msg, skip=1):
    '''issue warning with the message and the current stack, skipping the
    skip last entries'''
    ui.warn('%s at:\n' % msg)
    entries = traceback.extract_stack()[:-skip]
    fnmax = max(len(entry[0]) for entry in entries)
    for fn, ln, func, _text in entries:
        ui.warn(' %*s:%-4s in %s\n' % (fnmax, fn, ln, func))

def _checklock(repo):
    l = repo._lockref and repo._lockref()
    if l is None or not l.held:
        _warnstack(repo.ui, 'missing lock', skip=2)

def reposetup(ui, repo):
    orig = repo.__class__
    class lockcheckrepo(repo.__class__):
        def _writejournal(self, *args, **kwargs):
            _checklock(self)
            return orig._writejournal(self, *args, **kwargs)

        def transaction(self, *args, **kwargs):
            _checklock(self)
            return orig.transaction(self, *args, **kwargs)

        # TODO(durin42): kiilerix had a commented-out lock check in
        # _writebranchcache and _writerequirements

        def _tag(self, *args, **kwargs):
            _checklock(self)
            return orig._tag(self, *args, **kwargs)

        def write(self, *args, **kwargs):
            assert os.path.lexists(self._join('.hg/wlock'))
            return orig.write(self, *args, **kwargs)

    repo.__class__ = lockcheckrepo
