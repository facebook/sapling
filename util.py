import os
import pickle
import shutil

from mercurial import node

svn_subcommands = { }

def register_subcommand(name):
    def inner(fn):
        svn_subcommands[name] = fn
        return fn
    return inner


def generate_help():
    ret = ['', 'hg svn subcommand\n', 'Subcommands:\n']

    for name, func in sorted(svn_subcommands.items()):
        short_description = (func.__doc__ or '').split('\n')[0]
        ret.append(" %-10s  %s" % (name, short_description))

    return "\n".join(ret) + '\n'


def normalize_url(svn_url):
    while svn_url[-1] == '/':
        svn_url = svn_url[:-1]
    return svn_url


def wipe_all_files(hg_wc_path):
    files = [f for f in os.listdir(hg_wc_path) if f != '.hg']
    for f in files:
        f = os.path.join(hg_wc_path, f)
        if os.path.isdir(f):
            shutil.rmtree(f)
        else:
            os.remove(f)

REVMAP_FILE_VERSION = 1
def parse_revmap(revmap_filename):
    revmap = {}
    f = open(revmap_filename)
    ver = int(f.readline())
    if ver == 1:
        for l in f:
            revnum, node_hash, branch = l.split(' ', 2)
            if branch == '\n':
                branch = None
            else:
                branch = branch[:-1]
            revmap[int(revnum), branch] = node.bin(node_hash)
        f.close()
    else: #pragma: no cover
        print ('Your revmap was made by a newer version of hgsubversion.'
               ' Please upgrade.')
        raise NotImplementedError
    return revmap


class PrefixMatch(object):
    def __init__(self, prefix):
        self.p = prefix

    def files(self):
        return []

    def __call__(self, fn):
        return fn.startswith(self.p)

def outgoing_revisions(ui, repo, hg_editor, reverse_map, sourcerev):
    """Given a repo and an hg_editor, determines outgoing revisions for the
    current working copy state.
    """
    outgoing_rev_hashes = []
    if sourcerev in reverse_map:
        return
    sourcerev = repo[sourcerev]
    while (not sourcerev.node() in reverse_map
           and sourcerev.node() != node.nullid):
        outgoing_rev_hashes.append(sourcerev.node())
        sourcerev = sourcerev.parents()
        assert len(sourcerev) == 1
        sourcerev = sourcerev[0]
    if sourcerev.node() != node.nullid:
        return outgoing_rev_hashes

def build_extra(revnum, branch, uuid, subdir):
    # TODO this needs to be fixed with the new revmap
    extra = {}
    branchpath = 'trunk'
    if branch:
        extra['branch'] = branch
        branchpath = 'branches/%s' % branch
    if subdir and subdir[-1] == '/':
        subdir = subdir[:-1]
    if subdir and subdir[0] != '/':
        subdir = '/' + subdir
    extra['convert_revision'] = 'svn:%(uuid)s%(path)s@%(rev)s' % {
        'uuid': uuid,
        'path': '%s/%s' % (subdir , branchpath),
        'rev': revnum,
        }
    return extra


def is_svn_repo(repo):
    return os.path.exists(os.path.join(repo.path, 'svn'))
