# tiny extension to report phase changes during transaction

from __future__ import absolute_import

def reposetup(ui, repo):

    def reportphasemove(tr):
        for rev, move in sorted(tr.changes['phases'].iteritems()):
            if move[0] is None:
                ui.write(('test-debug-phase: new rev %d:  x -> %d\n'
                          % (rev, move[1])))
            else:
                ui.write(('test-debug-phase: move rev %d: %s -> %d\n'
                          % (rev, move[0], move[1])))

    class reportphaserepo(repo.__class__):
        def transaction(self, *args, **kwargs):
            tr = super(reportphaserepo, self).transaction(*args, **kwargs)
            tr.addpostclose('report-phase', reportphasemove)
            return tr

    repo.__class__ = reportphaserepo
