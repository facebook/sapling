# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# repo.py - Google Repo support for the convert extension (EXPERIMENTAL)

from __future__ import absolute_import

import datetime
import functools
import os
import pprint
import re
import xml.dom.minidom

from edenscm.mercurial import error, node as nodemod, pycompat
from edenscm.mercurial.i18n import _

from . import common


class gitutil(object):
    """Helper functions for dealing with git data"""

    OBJECT_TYPE_TREE = "tree"

    FILE_MODE_MASK_LINK = 0o020000
    FILE_MODE_MASK_GLOBAL_EXEC = 0o000100
    FILE_MODE_MASK_USER_EXEC = 0o000001

    GIT_FILE_STATUS_ADDED = "A"
    GIT_FILE_STATUS_COPIED = "C"
    GIT_FILE_STATUS_DELETED = "D"
    GIT_FILE_STATUS_MODIFIED = "M"
    GIT_FILE_STATUS_RENAMED = "R"
    GIT_FILE_STATUS_TYPE_CHANGED = "T"
    GIT_FILE_STATUS_UNMERGED = "U"
    GIT_FILE_STATUS_UNKNOWN = "X"

    FILE_MODE_LINK = "l"
    FILE_MODE_EXEC = "x"
    FILE_MODE_OTHER = ""

    catfilepipes = {}
    difftreepipes = {}

    @classmethod
    def getfilemodestr(cls, mode):
        """return value: the string representing the file's mode
        """
        if cls.islinkfilemode(mode):
            return cls.FILE_MODE_LINK

        if cls.isexecfilemode(mode):
            return cls.FILE_MODE_EXEC

        return cls.FILE_MODE_OTHER

    @classmethod
    def islinkfilemode(cls, mode):
        return (mode & cls.FILE_MODE_MASK_LINK) > 0

    @classmethod
    def isexecfilemode(cls, mode):
        return (
            mode & (cls.FILE_MODE_MASK_GLOBAL_EXEC | cls.FILE_MODE_MASK_USER_EXEC)
        ) > 0

    @classmethod
    def iscopystatus(cls, status):
        return status == cls.GIT_FILE_STATUS_COPIED

    @classmethod
    def _parsespecificchar(cls, s, c):
        if s[0] != c:
            raise ValueError(
                _('Parse error: Expected "%s", found "%s"') % (repr(c), repr(s[0]))
            )
        return s[1:]

    @classmethod
    def _parsemode(cls, s):
        if len(s) < 6:
            raise ValueError()
        return int(s[0:6], 8), s[6:]

    @classmethod
    def _parsehash(cls, s):
        if len(s) < 40:
            raise ValueError(_('Value "%s" is not long enough to contain a hash') % s)
        return s[0:40], s[40:]

    @classmethod
    def _parsestatus(cls, s):
        status, remainder = s[0], s[1:]
        if status not in [
            cls.GIT_FILE_STATUS_ADDED,
            cls.GIT_FILE_STATUS_COPIED,
            cls.GIT_FILE_STATUS_DELETED,
            cls.GIT_FILE_STATUS_MODIFIED,
            cls.GIT_FILE_STATUS_RENAMED,
            cls.GIT_FILE_STATUS_TYPE_CHANGED,
            cls.GIT_FILE_STATUS_UNMERGED,
            cls.GIT_FILE_STATUS_UNKNOWN,
        ]:
            raise ValueError(_('Status "%s" is invalid') % status)

        return status, remainder

    @classmethod
    def _parsepath(cls, s, separator):
        endindex = s.index(separator)
        return s[:endindex], s[endindex:]

    @classmethod
    def _parsedifftreeline(cls, linestr):
        remainder = cls._parsespecificchar(linestr, ":")
        srcmode, remainder = cls._parsemode(remainder)
        remainder = cls._parsespecificchar(remainder, " ")
        dstmode, remainder = cls._parsemode(remainder)
        remainder = cls._parsespecificchar(remainder, " ")
        srchash, remainder = cls._parsehash(remainder)
        remainder = cls._parsespecificchar(remainder, " ")
        dsthash, remainder = cls._parsehash(remainder)
        remainder = cls._parsespecificchar(remainder, " ")
        status, remainder = cls._parsestatus(remainder)
        if status in [cls.GIT_FILE_STATUS_COPIED, cls.GIT_FILE_STATUS_RENAMED] or (
            status == cls.GIT_FILE_STATUS_MODIFIED
            and remainder[0] not in ["\t", "\x00"]
        ):
            score, remainder = int(remainder[0:2]), remainder[2:]
        else:
            score = None
        fieldseparator, remainder = remainder[0], remainder[1:]
        if fieldseparator not in ["\t", "\x00"]:
            raise ValueError(
                _('Git parse error: "%s" is not a valid field separator')
                % fieldseparator
            )
        lineseparator = "\n" if fieldseparator == "\t" else "\x00"

        if status in [cls.GIT_FILE_STATUS_COPIED, cls.GIT_FILE_STATUS_RENAMED]:
            srcpath, remainder = cls._parsepath(remainder, fieldseparator)
            remainder = cls._parsespecificchar(remainder, fieldseparator)
            dstpath, remainder = cls._parsepath(remainder, lineseparator)
        else:
            dstpath, remainder = cls._parsepath(remainder, lineseparator)
            srcpath = dstpath
        if dstpath.find("\x00") > -1 or dstpath.find("\n") > -1:
            raise error.Abort(_('delimiter "%s"') % repr(dstpath))
        remainder = cls._parsespecificchar(remainder, lineseparator)

        return (
            {
                "source": {"mode": srcmode, "hash": srchash, "path": srcpath},
                "dest": {"mode": dstmode, "hash": dsthash, "path": dstpath},
                "status": status,
                "score": score,
            },
            remainder,
        )

    @classmethod
    def parsedifftree(cls, diffstr, expectedrev=None):
        """
        returns: [[source:{}, dest:{}, status:"", score:0]]
        """
        remainder = diffstr
        output = []
        parentdiffs = None
        while remainder:
            if remainder[0] == ":":
                diff, remainder = cls._parsedifftreeline(remainder)
                parentdiffs.append(diff)
            else:
                rev, remainder = cls._parsehash(remainder)
                if expectedrev is not None and rev != expectedrev:
                    raise error.Abort(
                        _("diff-tree out of sync! Expected %s, found %s")
                        % (expectedrev, rev)
                    )
                remainder = remainder[1:]  # remove separator
                parentdiffs = []
                output.append(parentdiffs)
        return output

    @classmethod
    def parsegitcommitraw(cls, commithash, commitstr, recode=None):
        """Takes the text of a git commit in raw format and builds a commit object based
        on it.
        """
        if not isinstance(commitstr, unicode):
            raise TypeError("parsegitcommitraw: commitstr must be a unicode string")
        if len(commitstr) == 0:
            raise ValueError("parsegitcommitraw: commitstr is empty")
        if recode is None:
            recode = lambda x: x

        if commitstr.startswith("commit"):
            (hashline, remainder) = commitstr.split("\n", 1)
        else:
            hashline = None
            commithash = commithash
            remainder = commitstr
        (treeline, remainder) = remainder.split("\n", 1)
        parentlines = []
        parentline = ""
        while remainder.startswith("parent"):
            (parentline, remainder) = remainder.split("\n", 1)
            parentlines.append(parentline)
        (authorline, remainder) = remainder.split("\n", 1)
        (committerline, remainder) = remainder.split("\n", 1)
        if remainder:
            _, remainder = remainder.split("\n", 1)  # blank line
            description = remainder
        else:
            description = ""

        cls.gitshowrawcommitregex = re.compile(r"^commit (?P<hash>[0-9a-f]+)\s*$")
        cls.gitshowrawtreeregex = re.compile(r"^tree (?P<hash>[0-9a-f]+)\s*$")
        cls.gitshowrawparentregex = re.compile(r"^parent (?P<hash>[0-9a-f]+)\s*$")
        cls.gitshowrawauthorregex = re.compile(
            r"^author (?P<username>[^<]+) <(?P<email>[^>]*)> (?P<time>\d*)"
            r" (?P<timezone>[+-]?\d{4})\s*$"
        )
        cls.gitshowrawcommitterregex = re.compile(
            r"^committer (?P<username>[^<]+) <(?P<email>[^>]*)> (?P<time>\d*)"
            r" (?P<timezone>[+-]?\d{4})\s*$"
        )

        if hashline:
            commithash = cls.gitshowrawcommitregex.match(hashline).group("hash")
        # treehash = cls.git_show_raw_tree_regex.match(treeline).group("hash")
        parents = []
        for line in parentlines:
            parentmatch = cls.gitshowrawparentregex.match(line)
            if parentmatch:
                parents.append(parentmatch.group("hash"))

        author = None
        date = datetime.datetime.utcnow()  # TODO: date should never be empty
        timezone = "+0000"
        authormatch = cls.gitshowrawauthorregex.match(authorline)
        if authormatch is not None:
            author = authormatch.group("email")
            date = datetime.datetime.fromtimestamp(
                float(authormatch.group("time")),
                None,  # TODO: Use time zone info from commit
            )
            timezone = authormatch.group("timezone")

        committermatch = cls.gitshowrawcommitterregex.match(committerline)
        if committermatch is not None:
            date = datetime.datetime.fromtimestamp(
                float(committermatch.group("time")),
                None,  # TODO: Use time zone info from commit
            )
            timezone = committermatch.group("timezone")

        date = date.strftime("%Y-%m-%dT%H:%M:%S") + timezone

        return common.commit(
            author=recode(author),
            date=recode(date),
            desc=recode(description),
            parents=[recode(parent) for parent in parents],
            rev=recode(commithash),
            extra={"git-hash": recode(commithash)},
            saverev=True,
        )

    @classmethod
    def _createcatfilepipe(cls, ui, path):
        """Initializes a catfile batch process for a particular git repository, complete
        with input, output and error pipes.

        path: the repo-relative path of the git repository
        return value: a tuple of the input, output and error streams for the catfile
        process
        """

        pipecommandline = common.commandline(ui, "git")
        catfilein, catfileout, catfileerr = pipecommandline._run3(
            "-C", path, "cat-file", "--batch"
        )
        return (catfilein, catfileout, catfileerr)

    @classmethod
    def _getcatfilepipe(cls, ui, path):
        """Gets a catfile pipe for a git project, creating it if needed

        ui: The UI context for accessing input/output
        path: the repo-relative path of the git repository
        return value: a tuple of streams: input, output, error
        """
        if path not in cls.catfilepipes:
            cls.catfilepipes[path] = cls._createcatfilepipe(ui, path)
        return cls.catfilepipes[path]

    @classmethod
    def catfilewithtype(cls, ui, path, version):
        """Uses the catfile pipe implementation to get the type and file contents for a
        particular object hash

        ui: The UI context for accessing input/output
        path: the repo-relative path of the git project containing the object
        version: the hash of the object to fetch
        return type: a tuple of the object type of the object, and a byte string for the
        object's contents
        """
        pipein, pipeout, pipeerr = cls._getcatfilepipe(ui, path)
        pipein.write(version)
        pipein.write("\n")
        pipein.flush()
        outheader = pipeout.readline()
        outhash, outtype, outbodysize = outheader.split()
        outbodybytes = pipeout.read(int(outbodysize))
        finaloutchar = pipeout.read(1)  # read out final newline
        if finaloutchar != "\n":
            raise error.Abort(
                _('cat-file(%s, %s) output ended with "%s" not \\n')
                % (path, version, finaloutchar)
            )
        return outtype, outbodybytes

    @classmethod
    def catfile(cls, ui, path, version):
        """Uses the catfile pipe implementation to get the file contents for a
        particular object hash
        """
        out_type, out_body = cls.catfilewithtype(ui, path, version)
        return out_body

    @classmethod
    def _createdifftreepipe(cls, ui, path):
        """
        path:
        returns: (in, out, err)
        """
        pipecommandline = common.commandline(ui, "git")
        pipein, pipeout, pipeerr = pipecommandline._run3(
            "-C", path, "diff-tree", "--stdin", "--root", "-z", "-m", "-r"
        )
        return pipein, pipeout, pipeerr

    @classmethod
    def _getdifftreepipe(cls, ui, path):
        """
        """
        if path not in cls.difftreepipes:
            cls.difftreepipes[path] = cls._createdifftreepipe(ui, path)
        return cls.difftreepipes[path]

    @classmethod
    def difftree(cls, ui, projectpath, treeishhash):
        """ Executes git diff-tree on a git project
        """
        pipein, pipeout, pipeerr = cls._getdifftreepipe(ui, projectpath)
        pipein.write(treeishhash)
        # Diff-tree in stdin mode doesn't use a clear way of separating the responses
        # for each input line. We need to put in a clear separator.
        pipein.write("\n\n")
        pipein.flush()

        # TODO: This won't be reliable for filenames with newlines. We need to parse a
        # whole change at a time in between looking for newlines
        outbody = pipeout.readline()

        return outbody


class repo_commandline(common.commandline):
    """Specialized command line implementation for the repo tool"""

    def __init__(self, ui, path):
        super(repo_commandline, self).__init__(ui, "repo")
        self.cwd = pycompat.getcwd()
        self.repopath = path

    def prerun(self):
        if self.repopath:
            os.chdir(self.repopath)

    def postrun(self):
        if self.cwd:
            os.chdir(self.cwd)
        self.cwd = None


class repo(object):
    """Represents a source code repository managed by the Google repo tool"""

    MANIFEST_FILENAME_DEFAULT = "default.xml"

    def __init__(self, ui, path):
        if not os.path.exists(os.path.join(path, ".repo")):
            raise common.NoRepo(_("%s does not look like a repo repository") % path)

        self.ui = ui
        self.path = path
        self.branches = None
        self.repocommandline = repo_commandline(ui, path)
        self.gitcommandline = common.commandline(ui, "git")

        self.repobranchsingleregex = re.compile(
            r"^(?P<checkedout>[* ])(?P<published>[pP ]) (?P<name>\S+)\s+|"
            r" in\ (?P<project>\S+)$"
        )
        self.repobranchmultistartregex = re.compile(
            r"^(?P<checkedout>[* ])(?P<published>[pP ]) (?P<name>\S+)\s+\| in:$"
        )

        self.projectpathindex = self._buildprojectmap()

        self.manifestprojectpath = os.path.dirname(
            os.path.realpath(os.path.join(path, ".repo/manifest.xml"))
        )
        if not os.path.exists(self.manifestprojectpath):
            raise error.Abort(
                _('Could not find manifest project path "%s"')
                % self.manifestprojectpath
            )

        self._fetchmanifestdata(self.manifestprojectpath)

    def _fetchmanifestdata(self, manifestprojectpath):
        self.manifestbranchcommithashes = {}
        for branchname in self.getbranches():
            projecthash, exitcode = self.gitcommandline.run(
                "-C", self.manifestprojectpath, "rev-parse", branchname
            )
            if exitcode > 0:
                raise error.Abort(_("rev-parse failed %d") % exitcode)
            self.manifestbranchcommithashes[branchname] = projecthash.strip()

        # Map repo manifest branch -> branch/commit hash for other projects
        # Do it by fetching the manifest for a particular branch, parsing the XML,
        # and pulling out the different branch versions for each project
        self.repoprojectbranches = {}
        for repobranch in self.getbranches():
            manifestdom = self.getmanifestdom(branchname)
            manifestprojects = self._parsemanifestxmldom(manifestdom)
            self.repoprojectbranches[repobranch] = {}
            for projectpath in manifestprojects.keys():
                projectname, commitname = manifestprojects[projectpath]
                commithash, exitcode = self.gitcommandline.run(
                    "-C", projectpath, "rev-parse", commitname
                )
                if exitcode > 0:
                    raise error.Abort(_("rev-parse failed %d") % exitcode)
                self.repoprojectbranches[repobranch][projectpath] = (
                    projectname,
                    commithash.strip(),
                )

        # This is the magic. It looks at all of the commits from the various projects
        # and sorts them into a unified ordering
        # unified_commits: {
        # string: [(project_name: string, commit_hash: string, timestamp: int)]
        # }
        self.unifiedcommits = {}
        self.unifiedprevioushashes = {}
        for repobranch in self.repoprojectbranches.keys():
            # unified_project_commis will be a list of all of the commits for a given
            # repo-branch, merged into a single commit history
            unifiedprojectcommits = []
            for projectpath in self.repoprojectbranches[repobranch].keys():
                projectname, commitname = self.repoprojectbranches[repobranch][
                    projectpath
                ]
                outputlines, exitcode = self.gitcommandline.runlines(
                    "-C", projectpath, "log", "--pretty=format:%H:%ct", commitname
                )
                if exitcode > 0:
                    raise error.Abort(_("failed to log"))

                for line in outputlines:
                    commithash, timestamp = line.split(":")
                    unifiedprojectcommits.append(
                        (projectpath, commithash, int(timestamp))
                    )

            # Sort the commits by date
            unifiedprojectcommits = sorted(
                unifiedprojectcommits, key=lambda projectcommit: projectcommit[2]
            )
            previoushash = nodemod.nullhex
            for project, commithash, timestamp in unifiedprojectcommits:
                self.unifiedprevioushashes[commithash] = previoushash
                previoushash = commithash
            self.unifiedcommits[repobranch] = unifiedprojectcommits

    def _parsemanifestxmlfile(self, manifestpath):
        manifestdom = xml.dom.minidom.parse(manifestpath)
        return self._parsemanifestxmldom(manifestdom)

    def _parsemanifestxmlstring(self, manifeststring):
        manifestdom = xml.dom.minidom.parseString(manifeststring)
        return self._parsemanifestxmldom(manifestdom)

    def _parsemanifestxmldom(self, manifestdom):
        """
        returns: a dictionary of repo-relative path to (project name, revision)
        """

        # manifestnode = manifestdom.getElementsByTagName("manifest")[0]
        # remotenode = manifestdom.getElementsByTagName("remote")[0]
        defaultnode = manifestdom.getElementsByTagName("default")[0]
        projectnodes = manifestdom.getElementsByTagName("project")

        projectbranches = {
            os.path.join(
                self.path,
                (projectnode.getAttribute("path") or projectnode.getAttribute("name")),
            ): (
                projectnode.getAttribute("name"),
                # TODO: Allow project-specific remote
                projectnode.getAttribute("revision")
                or (
                    defaultnode.getAttribute("remote")
                    + "/"
                    + defaultnode.getAttribute("revision")
                ),
            )
            for projectnode in projectnodes
        }
        return projectbranches

    def _buildprojectmap(self):
        """Generates the list of subprojects in the repo"""
        return self.list()

    def _splitpath(self, name):
        """Given a repo-relative file pathname, splits it into the basepath of the git
        repo, and the git-relative path for the file
        """

        projectmatches = [
            folder for folder in self.projectmap.keys() if name.startswith(folder)
        ]
        # if len(project_matches) > 1:
        #    self.ui.debug("project_matches:\n", project_matches)
        project = projectmatches[0]
        if project[-1:] != "/":
            project += "/"
        gitrelfilename = name[len(project) :]
        return (project, gitrelfilename)

    def forall(self, command):
        """Runs a command on all of the repository's git projects"""
        outputlines, exitcode = self.repocommandline.runlines(
            "forall", "-c", "%s" % command
        )
        if exitcode > 0:
            raise RuntimeError("forall error %d" % exitcode)
        return outputlines

    def forallbyproject(self, command):
        """Runs a command on all of the repository's git projects"""
        cmdoutput, exitcode = self.repocommandline.run(
            "forall", "-p", "-c", "%s" % command
        )
        if exitcode > 0:
            raise RuntimeError("forall error %d" % exitcode)

        linesbyproject = {}
        remainder = cmdoutput
        while len(remainder) > 0:  # Run all the projects
            line, remainder = remainder.split("\n", 1)
            currentproject = line[8:]  # text after "project "
            currentlist = []
            while len(remainder) > 0 and not remainder.startswith("\nproject"):
                line, remainder = remainder.split("\n", 1)
                currentlist.append(line)
            if remainder.startswith("\n"):
                _, remainder = remainder.split(
                    "\n", 1
                )  # consume empty line between projects
            linesbyproject[currentproject] = currentlist

        return linesbyproject

    def list(self):
        """Dictionary of the  the Git projects contained within the repo"""

        outputlines, exitcode = self.repocommandline.runlines("list")
        if exitcode > 0:
            raise RuntimeError(
                "Error code returned when fetching projects: %d" % exitcode
            )
        projectdict = {}
        for line in outputlines:
            relpath, projectname = line.split(" : ", 1)
            projectdict[relpath] = projectname[:-1]
            projectdict[relpath + "/"] = projectname[:-1]

        return projectdict

    def getbranches(self):
        """Lists the branches in this repo by name"""
        if not self.branches:
            self.branches = self._readbranches()
        return self.branches

    def _readbranches(self):
        outputlines, exitcode = self.gitcommandline.runlines(
            "-C", self.manifestprojectpath, "branch", "--remote"
        )
        if exitcode > 0:
            raise error.Abort(
                _("failed to get manifest project branches %d") % exitcode
            )
        branches = set()
        for line in outputlines:
            arrowindex = line.find(" ->")
            branches.add(line[2:arrowindex])
        return branches

    def getmanifestdom(self, version="HEAD", filename="default.xml"):
        """Fetches a repo manifest as parsed XML

        version: the commit hash or branch name of the manifest to fetch
        filename: the filename of the manifest to fetch
        returns: the contents of the manifest file as an XML DOM
        """
        manifeststring = self.getmanifest(version, filename)
        manifestdom = xml.dom.minidom.parseString(manifeststring)
        return manifestdom

    def getmanifest(self, version="HEAD", filename="default.xml"):
        """Fetches the contents of a repo manifest

        version: the commit hash or branch name of the manifest to fetch
        filename: the filename of the manifest to fetch
        returns: the contents of the manifest file as a string
        """
        output, exitcode = self.gitcommandline.run(
            "-C", self.manifestprojectpath, "show", "%s:%s" % (version, filename)
        )

        if exitcode > 0:
            raise error.Abort(_("Failed to get manifest"))

        return output


class repo_source(common.converter_source):
    """Reads commit data from a Google repo repository for the Mercurial convert
    extension.

    Config options:
      * repo.difftreecache: TODO
      * repo.enabledirred: Include commits that are remapped into the file tree TODO
      * repo.fullmerge: Use merge commits to tie together the various versions TODO

    Implementation notes: This class should be kept at a high-level of abstraction,
    saving the details of repo and git operations for the repo and git helper classes
    above.

    This implementation is required to convert between repo's multiple project model
    and Mercurials single unified repository model. The way we do this is with a hack:
    we import each commit in the source into multiple commits in the sink. This is
    important for bookkeeping because some of the commits will be modified as they
    are converted rather than being simply copied from the source to the sink. We
    convert each commit in one of three modes (a.k.a. variants):

      * ROOTED: We convert the commit as a simple copy from the source project. Because
      the source project is rooted at a subdirectory of the repo project, these commits
      have file paths relative to the git project, not the repo repository. These
      commits also have commit histories that are completely contained within the single
      git source project.
      * DIRRED: We convert the commit as a copy from the source project, with one big
      distinction: we modify each file path to include a prefix of the repo-relative git
      project directory. This means that these commits will look like they have correct
      repo-relative filenames, but they will still have commit histories that are
      limited to just the source git project.
      * UNIFIED: These commits are the final, merged versions of commits, they have
      repo-relative file names like the DIRRED commits above, and also have their commit
      histories merged with the commit histories of all of the other git projects within
      the same source repo. (The merging is based on branches and timestamps.)

    The convert_source interface is designed to pass around version values, to identify
    the commits to be imported. We have adapted this to our multi-import needs by
    prefixing the version/commit values with a "variant" prefix to distinguish which of
    the three convert modes above needs to be executed on this version of the commit.
    You will notice us using a "split" method to extract the subfields from these hybrid
    values and "join" to combine them back together again.
    """

    CONFIG_NAMESPACE = "convert"
    CONFIG_FULL_MERGE = "repo.fullmerge"  # Find a better name for this
    CONFIG_DIFFTREE_CACHE_ENABLED = "repo.difftreecache"
    CONFIG_DIRRED_ENABLED = "repo.enabledirred"

    VARIANT_ROOTED = "R"  # Used for commits migrated to root directory
    VARIANT_DIRRED = "D"  # Used for commits migrated to manifest directory
    VARIANT_UNIFIED = "U"  # Used for commits at manifest directory merged into a
    # single commit history for all Git repos

    FILECACHE_SIZE_MAX = 1000
    DIFFCACHE_SIZE_MAX = 1000

    FORMAT_UNIFIED_COMMIT_MESSAGE = "[MERGED] %s"

    def __init__(self, ui, repotype, path, revs=None):
        """
        raises common.NoRepo if the directory doesn't exist or isn't a Google repo
        """

        super(repo_source, self).__init__(ui, repotype, path, revs=revs)

        self._fullmergeenabled = self.ui.configbool(
            self.CONFIG_NAMESPACE, self.CONFIG_FULL_MERGE, default=True
        )
        self._difftreecacheenabled = self.ui.configbool(
            self.CONFIG_NAMESPACE, self.CONFIG_DIFFTREE_CACHE_ENABLED, default=True
        )
        self._dirredenabled = self.ui.configbool(
            self.CONFIG_NAMESPACE, self.CONFIG_DIRRED_ENABLED, default=True
        )

        self.srcencoding = "utf-8"  # TODO: Read from git source projects
        self.pprinter = pprint.PrettyPrinter()
        self.repo = repo(ui, path)
        self.repocommandline = repo_commandline(ui, path)
        self.gitcommandline = common.commandline(ui, "git")

        self.pathprojectindex = self.repo._buildprojectmap()
        self.projectpathindex = {
            project: path for path, project in self.pathprojectindex.iteritems()
        }
        self.commitprojectindex = self._buildcommitprojectmap()
        self.objecthashprojectindex = {}
        self.filecache = {}
        self._difftreecache = {}

    def before(self):
        """See converter_source.before"""
        self.ui.debug("before\n")
        # TODO: Validate input repo, e.g. all branches present
        # TODO: Look at the manifest for the repo
        pass

    def after(self):
        """See converter_source.after"""
        self.ui.debug("after\n")
        # TODO: Validate output somehow
        pass

    def getheads(self):
        """See common.converter_source.getheads"""
        projectheads = self.repo.forallbyproject("git rev-parse --branches --remotes")
        commithashes = set()
        for (project, projectcommithashes) in projectheads.items():
            commithashes.update(projectcommithashes)
        if len(commithashes) == 0:
            self.ui.warn(_("heads list for repo is empty\n"))
        if "" in commithashes:
            self.ui.warn(_("heads list contains the empty string\n"))

        # self.repo.getbranches()
        unifiedheads = [
            self.repo.unifiedcommits[branchname][-1][1]
            for branchname, commithash in self.repo.manifestbranchcommithashes.items()
            if branchname in self.repo.unifiedcommits
        ]

        # Register all of the source commits for each of the variants
        heads = (
            [
                self._joinrevfields(self.VARIANT_ROOTED, commithash)
                for commithash in commithashes
            ]
            + [
                self._joinrevfields(self.VARIANT_DIRRED, commithash)
                for commithash in commithashes
                if self._dirredenabled
            ]
            + [
                self._joinrevfields(self.VARIANT_UNIFIED, commithash)
                for commithash in unifiedheads
            ]
        )
        return heads

    def getfile(self, name, rev):
        """Overrides common.converter_source.getfile

        name: the name of the file
        rev: the Git object hash of a tree or blob in a diff
        returns: a tuple of the file's bytes with the mode
        """
        if rev == nodemod.nullhex:
            return None, None

        if (name, rev) in self.filecache:
            objecttype, filebytes = self.filecache[(name, rev)]
        else:
            projectpath = self.objecthashprojectindex[rev]
            fullpath = os.path.join(self.path, projectpath)
            objecttype, filebytes = gitutil.catfilewithtype(self.ui, fullpath, rev)

        if objecttype == gitutil.OBJECT_TYPE_TREE:
            return None, None

        if len(self.filecache) > self.FILECACHE_SIZE_MAX:
            self.filecache.pop(self.filecache.keys()[0])
        self.filecache[(name, rev)] = (objecttype, filebytes)

        mode = ""  # TODO

        return filebytes, mode

    def getchanges(self, version, full):
        """Overrides common.converter_source.getchanges

        version: a string representing a git commit hash
        full: if truthy, include unchanged files in the output changes
        returns: A tuple of data from the commit requested
          changes: a list of (path after diff, hash before diff)
          copies:  a dictionary of paths for any files copied {after: before}
          cleanp2: a set of filenames that are "clean against p2" (meaning?)
        """
        if full:
            raise error.Abort(_("convert from git does not support --full"))

        if version is None:
            raise ValueError("version may not be None")
        if not isinstance(version, basestring):
            raise TypeError(_("version must be a string"))
        if len(version) == 0:
            raise ValueError(_("verion must not be empty"))

        variant, commithash = self._splitrevfields(version)
        if commithash not in self.commitprojectindex:
            raise LookupError(
                _("could not find which project contains commit %s") % commithash
            )

        projectpath = self.commitprojectindex[commithash]

        if (
            self._difftreecacheenabled
            and (projectpath, commithash) in self._difftreecache
        ):
            difftree = self._difftreecache[(projectpath, commithash)]
        else:
            gitpath = os.path.join(self.path, projectpath)
            difftreeoutput = gitutil.difftree(self.ui, gitpath, commithash)
            difftree = gitutil.parsedifftree(difftreeoutput[0:-1], commithash)
            if len(self._difftreecache) > self.DIFFCACHE_SIZE_MAX:
                self._difftreecache.popitem()
            if self._difftreecacheenabled:
                self._difftreecache[(projectpath, commithash)] = difftree

            # TODO: Fix for multiple parents
            for parentdiff in difftree:
                for filediff in parentdiff:
                    # Keep track of which project contains these trees and blobs for later
                    self.objecthashprojectindex[
                        filediff["source"]["hash"]
                    ] = projectpath
                    self.objecthashprojectindex[filediff["dest"]["hash"]] = projectpath

        pathprefix = {
            self.VARIANT_ROOTED: "",
            self.VARIANT_DIRRED: projectpath,
            self.VARIANT_UNIFIED: projectpath,
        }[variant]

        changes = []
        copies = {}

        for parentdiff in difftree:
            for filediff in parentdiff:
                newpath = os.path.join(pathprefix, filediff["dest"]["path"])
                changes.append((newpath, filediff["dest"]["hash"]))

                if filediff["status"] in [
                    gitutil.GIT_FILE_STATUS_COPIED,
                    gitutil.GIT_FILE_STATUS_RENAMED,
                ]:
                    oldpath = os.path.join(pathprefix, filediff["source"]["path"])
                    copies[newpath] = oldpath

                    # Renamed files are represented as an addition and a removal along with
                    # an entry in `copies`.
                    if filediff["status"] == gitutil.GIT_FILE_STATUS_RENAMED:
                        changes.append((oldpath, nodemod.nullhex))

        cleanp2 = set()
        return (changes, copies, cleanp2)

    def getcommit(self, version):
        """Overrides common.converter_source.getcommit

        version: a git commit hash prefixed by the variant "R", "D" or "U"
        return value: a common.commit object representing the commit
        """
        if version is None:
            raise TypeError("getcommit: version must not be none")
        if not isinstance(version, basestring):
            raise TypeError("getcommit: version must be a string not %s" % version)
        if len(version) == 0:
            raise ValueError("getcommit: version must not be empty")

        variant, commithash = self._splitrevfields(version)
        # repo forall -C "git show <version> 2> /dev/null"
        if commithash not in self.commitprojectindex:
            raise error.Abort(
                _("Could not find project for rev %s,%s") % (version, commithash)
            )
        projectpath = self.commitprojectindex[commithash]
        if not projectpath:
            raise error.Abort(_("Project path is empty %s") % commithash)

        # full_path = self.path + "/" + project_path
        fullpath = os.path.join(self.path, projectpath)
        catfilebodybytes = gitutil.catfile(self.ui, fullpath, commithash)
        catfilebodystr = catfilebodybytes.decode(self.srcencoding, errors="replace")

        commit = gitutil.parsegitcommitraw(commithash, catfilebodystr, self.recode)

        if variant == self.VARIANT_UNIFIED:
            previoushash = self.repo.unifiedprevioushashes[commit.rev]
            # self.ui.note('Previous hash for %s is %s\n' % (commit.rev, previous_hash))
            commit.desc = self.FORMAT_UNIFIED_COMMIT_MESSAGE % commit.desc
            if previoushash == nodemod.nullhex:
                commit.parents = []
            else:
                parentversion = self._joinrevfields(self.VARIANT_UNIFIED, previoushash)
                commit.parents = [parentversion]
            # Tie the dirred version back to directory-located version
            if self._dirredenabled:
                dirredhash = self._joinrevfields(self.VARIANT_DIRRED, commit.rev)
                commit.extra["dirred_hash"] = dirredhash
            rootedhash = self._joinrevfields(self.VARIANT_ROOTED, commit.rev)
            commit.extra["rooted_hash"] = rootedhash
            if self._fullmergeenabled:
                if self._dirredenabled:
                    commit.parents.append(dirredhash)
                else:
                    commit.parents.append(rootedhash)
        elif variant == self.VARIANT_DIRRED:
            commit.parents = [
                self._joinrevfields(variant, parenthash)
                for parenthash in commit.parents
            ]
            # Tie the dirred version back to rooted version
            rootedhash = self._joinrevfields(self.VARIANT_ROOTED, commit.rev)
            commit.extra["rooted_hash"] = rootedhash
            if self._fullmergeenabled:
                parentversion = rootedhash
                commit.parents.append(parentversion)
        else:
            commit.parents = [
                self._joinrevfields(variant, parenthash)
                for parenthash in commit.parents
            ]

        # Modify the commit's parent IDs to include the variant prefix
        commit.rev = self._joinrevfields(variant, commit.rev)
        commit.extra["convert_variant"] = variant
        commit.extra["source_project"] = projectpath

        return commit

    def numcommits(self):
        """See common.converter_source.numcommits"""
        output = self.repo.forallbyproject("git rev-list --all --count")
        sumfn = lambda lines, project: lines + int(output[project][0])
        rawcount = functools.reduce(sumfn, output, 0)
        return 3 * rawcount  # 1 for each of rooted, dirred and unified

    def gettags(self):
        """See common.converter_source.gettags"""
        # TODO: Convert to manifest tags only?
        # tagoutput = self.repo.forallbyproject("git tag")
        return []

    def getchangedfiles(self, rev, i):
        """See common.converter_source.getchangedfiles"""
        if rev is None:
            raise ValueError(_("version may not be None"))
        if not isinstance(rev, basestring):
            raise TypeError(_("version must be a string"))
        if len(rev) == 0:
            raise ValueError(_("verion must not be empty"))

        if rev not in self.commitprojectindex:
            raise LookupError(
                _("could not find which project contains version %s") % rev
            )

        projectpath = self.commitprojectindex[rev]

        revspecifier = None if i is None else ("%s^%d" % (rev, i))

        difftreeoutput, exitcode = self.gitcommandline.run(
            "-C",
            self.path,
            "-C",
            projectpath,
            "diff-tree",
            "--root",
            "-z",
            "-m",
            revspecifier,
            rev,
        )
        if exitcode > 0:
            raise error.Abort(_("diff-tree failed with %d") % exitcode)

        filediffs = gitutil.parsedifftree(difftreeoutput, rev)
        return [filediff["source"]["path"] for filediff in filediffs]

    def converted(self, rev, sinkrev):
        """See common.converter_source.converted"""
        # self.ui.debug("Conversion completed: %s -> %s\n" % (rev, sinkrev))
        # TODO: Log to some persistent place
        pass

    def getbookmarks(self):
        """See common.converter_source.getbookmarks"""
        # self.repo.getbranches()
        bookmarks = {
            branchname: self._joinrevfields(self.VARIANT_UNIFIED, commithashes[-1][1])
            for branchname, commithashes in self.repo.unifiedcommits.items()
        }
        return bookmarks

    def _buildcommitprojectmap(self):
        """Builds a map of git commit hashes to which git project contains them"""
        revlistout = self.repo.forallbyproject("git rev-list --all")
        # git_porcelain.rev_list?

        revprojectmap = {
            rev: projectpath for projectpath, revs in revlistout.items() for rev in revs
        }
        return revprojectmap

    def _joinrevfields(self, variant, commithash):
        """Combine the variant and commit hash into a single revision identifier

        return value: a single value that represents both the variant and commit hash
        """
        return variant + commithash

    def _splitrevfields(self, variantrev):
        """Split the variant field from the commit hash field in a revision

        return value: a tuple of the type of revision and the commit hash
        """
        return variantrev[0:1], variantrev[1:]
