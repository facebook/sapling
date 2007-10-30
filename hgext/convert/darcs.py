# darcs support for the convert extension

from common import NoRepo, commit, converter_source, checktool
from mercurial.i18n import _
from mercurial import util
import os, shutil, tempfile

# The naming drift of ElementTree is fun!

try: from xml.etree.cElementTree import ElementTree
except ImportError:
    try: from xml.etree.ElementTree import ElementTree
    except ImportError:
        try: from elementtree.cElementTree import ElementTree
        except ImportError:
            try: from elementtree.ElementTree import ElementTree
            except ImportError: ElementTree = None


class darcs_source(converter_source):
    def __init__(self, ui, path, rev=None):
        super(darcs_source, self).__init__(ui, path, rev=rev)

        if not os.path.exists(os.path.join(path, '_darcs', 'inventory')):
            raise NoRepo("couldn't open darcs repo %s" % path)

        checktool('darcs')

        if ElementTree is None:
            raise util.Abort(_("Python ElementTree module is not available"))

        self.path = os.path.realpath(path)

        self.lastrev = None
        self.changes = {}
        self.parents = {}
        self.tags = {}

    def before(self):
        self.tmppath = tempfile.mkdtemp(
            prefix='convert-' + os.path.basename(self.path) + '-')
        output, status = self.run('init', repodir=self.tmppath)
        self.checkexit(status)

        tree = self.xml('changes', '--xml-output', '--summary')
        tagname = None
        child = None
        for elt in tree.findall('patch'):
            node = elt.get('hash')
            name = elt.findtext('name', '')
            if name.startswith('TAG '):
                tagname = name[4:].strip()
            elif tagname is not None:
                self.tags[tagname] = node
                tagname = None
            self.changes[node] = elt
            self.parents[child] = [node]
            child = node
        self.parents[child] = []

    def after(self):
        self.ui.debug('cleaning up %s\n' % self.tmppath)
        shutil.rmtree(self.tmppath, ignore_errors=True)

    def _run(self, cmd, *args, **kwargs):
        cmdline = ['darcs', cmd, '--repodir', kwargs.get('repodir', self.path)]
        cmdline += args
        cmdline = [util.shellquote(arg) for arg in cmdline]
        cmdline += ['<', util.nulldev]
        cmdline = util.quotecommand(' '.join(cmdline))
        self.ui.debug(cmdline, '\n')
        return os.popen(cmdline, 'r')

    def run(self, cmd, *args, **kwargs):
        fp = self._run(cmd, *args, **kwargs)
        output = fp.read()
        return output, fp.close()

    def checkexit(self, status, output=''):
        if status:
            if output:
                self.ui.warn(_('darcs error:\n'))
                self.ui.warn(output)
            msg = util.explain_exit(status)[0]
            raise util.Abort(_('darcs %s') % msg)
        
    def xml(self, cmd, *opts):
        etree = ElementTree()
        fp = self._run(cmd, *opts)
        etree.parse(fp)
        self.checkexit(fp.close())
        return etree.getroot()

    def getheads(self):
        return self.parents[None]

    def getcommit(self, rev):
        elt = self.changes[rev]
        date = util.strdate(elt.get('local_date'), '%a %b %d %H:%M:%S %Z %Y')
        desc = elt.findtext('name') + '\n' + elt.findtext('comment', '')
        return commit(author=elt.get('author'), date=util.datestr(date),
                      desc=desc.strip(), parents=self.parents[rev])

    def pull(self, rev):
        output, status = self.run('pull', self.path, '--all',
                                  '--match', 'hash %s' % rev,
                                  '--no-test', '--no-posthook',
                                  '--external-merge', '/bin/false',
                                  repodir=self.tmppath)
        if status:
            if output.find('We have conflicts in') == -1:
                self.checkexit(status, output)
            output, status = self.run('revert', '--all', repodir=self.tmppath)
            self.checkexit(status, output)

    def getchanges(self, rev):
        self.pull(rev)
        copies = {}
        changes = []
        for elt in self.changes[rev].find('summary').getchildren():
            if elt.tag in ('add_directory', 'remove_directory'):
                continue
            if elt.tag == 'move':
                changes.append((elt.get('from'), rev))
                copies[elt.get('from')] = elt.get('to')
            else:
                changes.append((elt.text.strip(), rev))
        changes.sort()
        self.lastrev = rev
        return changes, copies

    def getfile(self, name, rev):
        if rev != self.lastrev:
            raise util.Abort(_('internal calling inconsistency'))
        return open(os.path.join(self.tmppath, name), 'rb').read()

    def getmode(self, name, rev):
        mode = os.lstat(os.path.join(self.tmppath, name)).st_mode
        return (mode & 0111) and 'x' or ''

    def gettags(self):
        return self.tags
