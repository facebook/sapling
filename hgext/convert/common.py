# common code for the convert extension

class NoRepo(Exception): pass

class commit(object):
    def __init__(self, **parts):
        for x in "author date desc parents".split():
            if not x in parts:
                raise util.Abort("commit missing field %s" % x)
        self.__dict__.update(parts)

class converter_source(object):
    """Conversion source interface"""

    def __init__(self, ui, path, rev=None):
        """Initialize conversion source (or raise NoRepo("message")
        exception if path is not a valid repository)"""
        raise NotImplementedError()

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
        """Return sorted list of (filename, id) tuples for all files changed in rev.

        id just tells us which revision to return in getfile(), e.g. in
        git it's an object hash."""
        raise NotImplementedError()

    def getcommit(self, version):
        """Return the commit object for version"""
        raise NotImplementedError()

    def gettags(self):
        """Return the tags as a dictionary of name: revision"""
        raise NotImplementedError()

    def recode(self, s, encoding=None):
        if not encoding:
            encoding = hasattr(self, 'encoding') and self.encoding or 'utf-8'
            
        try:
            return s.decode(encoding).encode("utf-8")
        except:
            try:
                return s.decode("latin-1").encode("utf-8")
            except:
                return s.decode(encoding, "replace").encode("utf-8")

class converter_sink(object):
    """Conversion sink (target) interface"""

    def __init__(self, ui, path):
        """Initialize conversion sink (or raise NoRepo("message")
        exception if path is not a valid repository)"""
        raise NotImplementedError()

    def getheads(self):
        """Return a list of this repository's heads"""
        raise NotImplementedError()

    def mapfile(self):
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
