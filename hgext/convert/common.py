# common code for the convert extension
import base64
import cPickle as pickle

def encodeargs(args):
    def encodearg(s):
        lines = base64.encodestring(s)
        lines = [l.splitlines()[0] for l in lines]
        return ''.join(lines)

    s = pickle.dumps(args)
    return encodearg(s)

def decodeargs(s):
    s = base64.decodestring(s)
    return pickle.loads(s)

class NoRepo(Exception): pass

SKIPREV = 'SKIP'

class commit(object):
    def __init__(self, author, date, desc, parents, branch=None, rev=None,
                 extra={}):
        self.author = author or 'unknown'
        self.date = date or '0 0'
        self.desc = desc
        self.parents = parents
        self.branch = branch
        self.rev = rev
        self.extra = extra

class converter_source(object):
    """Conversion source interface"""

    def __init__(self, ui, path, rev=None):
        """Initialize conversion source (or raise NoRepo("message")
        exception if path is not a valid repository)"""
        self.ui = ui
        self.path = path
        self.rev = rev

        self.encoding = 'utf-8'

    def before(self):
        pass

    def after(self):
        pass

    def setrevmap(self, revmap, order):
        """set the map of already-converted revisions
        
        order is a list with the keys from revmap in the order they
        appear in the revision map file."""
        pass

    def getheads(self):
        """Return a list of this repository's heads"""
        raise NotImplementedError()

    def getfile(self, name, rev):
        """Return file contents as a string"""
        raise NotImplementedError()

    def getmode(self, name, rev):
        """Return file mode, eg. '', 'x', or 'l'"""
        raise NotImplementedError()

    def getchanges(self, version):
        """Returns a tuple of (files, copies)
        Files is a sorted list of (filename, id) tuples for all files changed
        in version, where id is the source revision id of the file.

        copies is a dictionary of dest: source
        """
        raise NotImplementedError()

    def getcommit(self, version):
        """Return the commit object for version"""
        raise NotImplementedError()

    def gettags(self):
        """Return the tags as a dictionary of name: revision"""
        raise NotImplementedError()

    def recode(self, s, encoding=None):
        if not encoding:
            encoding = self.encoding or 'utf-8'

        if isinstance(s, unicode):
            return s.encode("utf-8")
        try:
            return s.decode(encoding).encode("utf-8")
        except:
            try:
                return s.decode("latin-1").encode("utf-8")
            except:
                return s.decode(encoding, "replace").encode("utf-8")

    def getchangedfiles(self, rev, i):
        """Return the files changed by rev compared to parent[i].
    
        i is an index selecting one of the parents of rev.  The return
        value should be the list of files that are different in rev and
        this parent.

        If rev has no parents, i is None.
    
        This function is only needed to support --filemap
        """
        raise NotImplementedError()

class converter_sink(object):
    """Conversion sink (target) interface"""

    def __init__(self, ui, path):
        """Initialize conversion sink (or raise NoRepo("message")
        exception if path is not a valid repository)

        created is a list of paths to remove if a fatal error occurs
        later"""
        self.ui = ui
        self.path = path
        self.created = []

    def getheads(self):
        """Return a list of this repository's heads"""
        raise NotImplementedError()

    def revmapfile(self):
        """Path to a file that will contain lines
        source_rev_id sink_rev_id
        mapping equivalent revision identifiers for each system."""
        raise NotImplementedError()

    def authorfile(self):
        """Path to a file that will contain lines
        srcauthor=dstauthor
        mapping equivalent authors identifiers for each system."""
        return None

    def putfile(self, f, e, data):
        """Put file for next putcommit().
        f: path to file
        e: '', 'x', or 'l' (regular file, executable, or symlink)
        data: file contents"""
        raise NotImplementedError()

    def delfile(self, f):
        """Delete file for next putcommit().
        f: path to file"""
        raise NotImplementedError()

    def putcommit(self, files, parents, commit):
        """Create a revision with all changed files listed in 'files'
        and having listed parents. 'commit' is a commit object containing
        at a minimum the author, date, and message for this changeset.
        Called after putfile() and delfile() calls. Note that the sink
        repository is not told to update itself to a particular revision
        (or even what that revision would be) before it receives the
        file data."""
        raise NotImplementedError()

    def puttags(self, tags):
        """Put tags into sink.
        tags: {tagname: sink_rev_id, ...}"""
        raise NotImplementedError()

    def setbranch(self, branch, pbranches):
        """Set the current branch name. Called before the first putfile
        on the branch.
        branch: branch name for subsequent commits
        pbranches: (converted parent revision, parent branch) tuples"""
        pass

    def setfilemapmode(self, active):
        """Tell the destination that we're using a filemap

        Some converter_sources (svn in particular) can claim that a file
        was changed in a revision, even if there was no change.  This method
        tells the destination that we're using a filemap and that it should
        filter empty revisions.
        """
        pass
