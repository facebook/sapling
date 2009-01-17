#!/usr/bin/env python
import re

from mercurial import patch

import util
import hg_delta_editor

b_re = re.compile(r'^\+\+\+ b\/([^\n]*)', re.MULTILINE)
a_re = re.compile(r'^--- a\/([^\n]*)', re.MULTILINE)
devnull_re = re.compile(r'^([-+]{3}) /dev/null', re.MULTILINE)
header_re = re.compile(r'^diff --git .* b\/(.*)', re.MULTILINE)
newfile_devnull_re = re.compile(r'^--- /dev/null\n\+\+\+ b/([^\n]*)',
                                re.MULTILINE)
def filterdiff(diff, base_revision):
    diff = newfile_devnull_re.sub(r'--- \1\t(revision 0)' '\n'
                                  r'+++ \1\t(working copy)',
                                  diff)
    diff = a_re.sub(r'--- \1'+ ('\t(revision %d)' % base_revision), diff)
    diff = b_re.sub(r'+++ \1' + '\t(working copy)', diff)
    diff = devnull_re.sub(r'\1 /dev/null' '\t(working copy)', diff)

    diff = header_re.sub(r'Index: \1' + '\n' + ('=' * 67), diff)
    return diff


def diff_command(ui, repo, hg_repo_path, **opts):
    """show a diff of the most recent revision against its parent from svn
    """
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    parent = repo.parents()[0]
    o_r = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, parent.node())
    if o_r:
        parent = repo[o_r[-1]].parents()[0]
    base_rev, _junk = svn_commit_hashes[parent.node()]
    it = patch.diff(repo, parent.node(), None,
                    opts=patch.diffopts(ui, opts={'git': True,
                                                  'show_function': False,
                                                  'ignore_all_space': False,
                                                  'ignore_space_change': False,
                                                  'ignore_blank_lines': False,
                                                  'unified': True,
                                                  'text': False,
                                                  }))
    ui.write(filterdiff(''.join(it), base_rev))
diff_command = util.register_subcommand('diff')(diff_command)
