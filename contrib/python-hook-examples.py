'''
Examples of useful python hooks for Mercurial.
'''
from mercurial import patch, util

def diffstat(ui, repo, **kwargs):
    '''Example usage:

    [hooks]
    commit.diffstat = python:/path/to/this/file.py:diffstat
    changegroup.diffstat = python:/path/to/this/file.py:diffstat
    '''
    if kwargs.get('parent2'):
        return
    node = kwargs['node']
    first = repo[node].p1().node()
    if 'url' in kwargs:
        last = repo['tip'].node()
    else:
        last = node
    diff = patch.diff(repo, first, last)
    ui.write(patch.diffstat(util.iterlines(diff)))
