#!/usr/bin/python
import re
import os
import urllib

import util


b_re = re.compile(r'^\+\+\+ b\/([^\n]*)', re.MULTILINE)
a_re = re.compile(r'^--- a\/([^\n]*)', re.MULTILINE)
devnull_re = re.compile(r'^([-+]{3}) /dev/null', re.MULTILINE)
header_re = re.compile(r'^diff --git .* b\/(.*)', re.MULTILINE)
newfile_devnull_re = re.compile(r'^--- /dev/null\n\+\+\+ b/([^\n]*)',
                                re.MULTILINE)


def formatrev(rev):
    if rev == -1:
        return '\t(working copy)'
    return '\t(revision %d)' % rev


def filterdiff(diff, oldrev, newrev):
    diff = newfile_devnull_re.sub(r'--- \1\t(revision 0)' '\n'
                                  r'+++ \1\t(working copy)',
                                  diff)
    oldrev = formatrev(oldrev)
    newrev = formatrev(newrev)
    diff = a_re.sub(r'--- \1'+ oldrev, diff)
    diff = b_re.sub(r'+++ \1' + newrev, diff)
    diff = devnull_re.sub(r'\1 /dev/null\t(working copy)', diff)
    diff = header_re.sub(r'Index: \1' + '\n' + ('=' * 67), diff)
    return diff


def parentrev(ui, repo, meta, hashes):
    """Find the svn parent revision of the repo's dirstate.
    """
    workingctx = repo.parents()[0]
    outrev = util.outgoing_revisions(repo, hashes, workingctx.node())
    if outrev:
        workingctx = repo[outrev[-1]].parents()[0]
    return workingctx


def islocalrepo(url):
    if not url.startswith('file:///'):
        return False
    if '#' in url.split('/')[-1]: # strip off #anchor
        url = url[:url.rfind('#')]
    path = url[len('file://'):]
    path = urllib.url2pathname(path).replace(os.sep, '/')
    while '/' in path:
        if reduce(lambda x,y: x and y,
                  map(lambda p: os.path.exists(os.path.join(path, p)),
                      ('hooks', 'format', 'db', ))):
            return True
        path = path.rsplit('/', 1)[0]
    return False

def issvnurl(url):
    for scheme in ('svn', 'http', 'https', 'svn+ssh'):
        if url.startswith(scheme + '://'):
            return True
    return islocalrepo(url)
