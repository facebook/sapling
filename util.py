import os
import shutil

from mercurial import hg
from mercurial import node
from mercurial import util as hgutil

svn_subcommands = { }
def register_subcommand(name):
    def inner(fn):
        svn_subcommands[name] = fn
        return fn
    return inner

svn_commands_nourl = set()
def command_needs_no_url(fn):
    svn_commands_nourl.add(fn)
    return fn


def version(ui):
    """Guess the version of hgsubversion.
    """
    # TODO make this say something other than "unknown" for installed hgsubversion
    repo = hg.repository(ui, os.path.dirname(__file__))
    ver = repo.dirstate.parents()[0]
    return node.hex(ver)[:12]


def generate_help():
    ret = ['hg svn ...', '',
           'subcommands for Subversion integration', '',
           'list of subcommands:', '']

    for name, func in sorted(svn_subcommands.items()):
        short_description = (func.__doc__ or '').splitlines()[0]
        ret.append(" %-10s  %s" % (name, short_description))

    return "\n".join(ret) + '\n'


def normalize_url(svn_url):
    return svn_url.rstrip('/')


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
        if len(sourcerev) != 1:
            raise hgutil.Abort("Sorry, can't find svn parent of a merge revision.")
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

default_commit_msg = '*** empty log message ***'

def describe_revision(ui, r):
    try:
        msg = [s for s in map(str.strip, r.message.splitlines()) if s][0]
    except:
        msg = default_commit_msg

    ui.status(('[r%d] %s: %s' % (r.revnum, r.author, msg))[:80] + '\n')

def describe_commit(ui, h, b):
    ui.note(' committed to "%s" as %s\n' % ((b or 'default'), node.short(h)))


def swap_out_encoding(new_encoding="UTF-8"):
    """ Utility for mercurial incompatibility changes, can be removed after 1.3"""
    try:
        from mercurial import encoding
        old = encoding.encoding
        encoding.encoding = new_encoding
    except ImportError:
        old = hgutil._encoding
        hgutil._encoding = new_encoding
    return old
