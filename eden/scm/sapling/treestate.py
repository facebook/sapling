# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""tree-based dirstate combined with other (ex. fsmonitor) states"""

import errno
import io

from bindings import treestate

from . import error, node, txnutil, util
from .i18n import _

# header after the first 40 bytes of dirstate.
HEADER = b"\ntreestate\n\0"


class _overlaydict(dict):
    def __init__(self, lookup, *args, **kwargs):
        super(_overlaydict, self).__init__(*args, **kwargs)
        self.lookup = lookup

    def get(self, key, default=None):
        s = super(_overlaydict, self)
        if s.__contains__(key):
            return s.__getitem__(key)
        r = self.lookup(key)
        if r is not None:
            return r
        return default

    def __getitem__(self, key):
        s = super(_overlaydict, self)
        if s.__contains__(key):
            return s[key]
        r = self.lookup(key)
        if r is not None:
            return r
        raise KeyError(key)


def _packmetadata(dictobj):
    result = []
    for k, v in dictobj.items():
        if not v:
            continue
        entry = "%s=%s" % (k, v)
        if "=" in k or "\0" in entry:
            raise error.ProgrammingError("illegal metadata entry: %r" % entry)
        result.append(entry)
    return "\0".join(result).encode()


def _unpackmetadata(data):
    data = data.decode()
    return dict(entry.split("=", 1) for entry in data.split("\0") if "=" in entry)


class treestatemap:
    """Map encapsulating the dirstate's contents.

    The dirstate contains the following state:

    - `identity` is the identity of the dirstate file, which can be used to
      detect when changes have occurred to the dirstate file.

    - `parents` is a pair containing the parents of the working copy. The
      parents are updated by calling `setparents`.

    - the state map maps filenames to tuples of (state, mode, size, mtime),
      where state is a single character representing 'normal', 'added',
      'removed', or 'merged'. It is read by treating the dirstate as a
      dict.  File state is updated by calling the `addfile`, `removefile` and
      `untrackfile` methods.

    - `copymap` maps destination filenames to their source filename.

    The dirstate also provides the following views onto the state:

    - `nonnormalset` is a set of the filenames that have state other
      than 'normal', or are normal but have an mtime of -1 ('normallookup').

    - `otherparentset` is a set of the filenames that are marked as coming
      from the second parent when the dirstate is currently being merged.

    - `filefoldmap` is a dict mapping normalized filenames to the denormalized
      form that they appear as in the dirstate.

    - `dirfoldmap` is a dict mapping normalized directory names to the
      denormalized form that they appear as in the dirstate.

    Treestatemap is drop-in replacement for previous dirstatemap, with
    more abilities like also track fsmonitor state.

    The treestate files are stored at ".hg/treestate/<uuid>". It uses a
    Rust-backed append-only map which tracks detailed information in one tree,
    and maintains aggregated states at each tree, so copymap, nonnormalset,
    otherparentset do not need to be tracked separately, and can be calculated
    in O(log N) time. It also stores a "metadata" string, which usually are p1,
    p2, and watchman clock.

    The first 40 bytes of ".hg/dirstate" remains compatible with earlier
    Mercurial, and the remaining bytes of ".hg/dirstate" contains the "<uuid>"
    and an offset.
    """

    def __init__(self, ui, vfs, root, rusttreestate, importdirstate=None):
        self._ui = ui
        self._vfs = vfs
        self._root = root
        self._read(rusttreestate)
        if importdirstate:
            # Import from an old dirstate
            self.clear()
            self.setparents(importdirstate.parents())
            self._tree.importmap(importdirstate._map)
            # Import copymap
            copymap = importdirstate.copies()
            for dest, src in copymap.items():
                self.copy(src, dest)
        assert len(self._parents) == 2

    @property
    def copymap(self):
        result = {}
        for path in self._tree.walk(treestate.COPIED, 0):
            copied = self._get(path)[-1]
            if not copied:
                raise error.Abort(
                    _(
                        "working directory state appears "
                        "damaged (wrong copied information)!"
                    )
                )
            result[path] = copied
        return result

    def clear(self):
        self._threshold = 0
        self.setparents(node.nullid, node.nullid)

        # use a new file
        self._rootid = self._tree.reset()

    def items(self):
        return ((k, self[k]) for k in self.keys())

    def __iter__(self):
        return iter(self.keys())

    def __len__(self):
        return len(self._tree)

    def get(self, key, default=None):
        entry = self._get(key)
        if entry is None or len(entry) != 5:
            return default
        flags, mode, size, mtime, _copied = entry

        # Python depends on size=-2 to infer P2 activity. Treestate
        # tracks P2 separate from size, so the size could be set to a
        # value other than -2.
        if flags & treestate.EXIST_P2:
            size = -2

        # convert flags to Mercurial dirstate state
        state = treestate.tohgstate(flags)
        return (state, mode, size, mtime)

    def __contains__(self, key):
        # note: this returns False for files with "?" state.
        return key in self._tree

    def __getitem__(self, key):
        result = self.get(key)
        if result is None:
            raise KeyError(key)
        return result

    def keys(self, prefix=None):
        # Exclude untracked files, since the matcher interface expects __iter__
        # to not include untracked files.
        return self._tree.tracked(prefix or "")

    def matches(self, match):
        """Filter tracked files by a matcher"""
        return self._tree.matches(match)

    def preload(self):
        pass

    def addfile(self, f, oldstate, state, mode, size, mtime):
        """Add a tracked file to the dirstate."""
        if state == "n":
            if size == -2:
                state = treestate.EXIST_P2 | treestate.EXIST_NEXT
            else:
                state = treestate.EXIST_P1 | treestate.EXIST_NEXT
        elif state == "m":
            state = treestate.EXIST_P1 | treestate.EXIST_P2 | treestate.EXIST_NEXT
        elif state == "a":
            state = treestate.EXIST_NEXT
        else:
            raise error.ProgrammingError("unknown addfile state: %s" % state)
        # TODO: figure out whether "copied" needs to be preserved here.
        self._tree.insert(f, state, mode, size, mtime, None)

    def removefile(self, f, oldstate, size):
        """
        Mark a file as removed in the treestate.
        """
        existing = self._get(f)
        if existing:
            # preserve "copied" information
            state, mode, size, mtime, copied = existing
            mode = 0
            mtime = -1
            # note: do not reset "size" if it is a special value (ex. -2).
            # some old code still depends on that. but do reset it since
            # some tests expect size to be 0.
            if size > 0:
                size = 0
            state ^= state & treestate.EXIST_NEXT
        else:
            state = 0
            copied = None
            mode = 0
            mtime = -1
            size = 0
        self._tree.insert(f, state, mode, size, mtime, copied)

    def deletefile(self, f, oldstate):
        """
        Deletes the file from the treestate, implying it doesn't exist on disk
        anymore and need not be inspected again unless watchman mentions it.
        """
        return self._tree.remove(f)

    def untrackfile(self, f, oldstate):
        """
        Removes the state marking a file as tracked, but leaves it in the
        treestate for future inspection.
        """
        if not self._clock:
            # If watchman clock is not set, watchman is not used, drop
            # untracked files directly. This is also correct if watchman
            # clock is reset to empty, since the next query will do a full
            # crawl.
            return self._tree.remove(f)
        else:
            # If watchman is used, treestate tracks "untracked" files before
            # watchman clock. So only remove EXIST_* bits and copy information
            # from the file. fsmonitor will do a stat check and drop(real=True)
            # later.
            #
            # Typically, dropfile is used in 2 cases:
            # - "hg forget": mark the file as "untracked".
            # - "hg update": remove files only tracked by old commit.
            entry = self._get(f)
            if not entry:
                return False
            else:
                state, mode, size, mtime, copied = entry
                copied = None
                state ^= state & (
                    treestate.EXIST_NEXT
                    | treestate.EXIST_P1
                    | treestate.EXIST_P2
                    | treestate.COPIED
                )
                state |= treestate.NEED_CHECK
                self._tree.insert(f, state, mode, size, mtime, copied)
                return True

    def clearambiguoustimes(self, now):
        self._tree.invalidatemtime(now)

    def nonnormalentries(self):
        return (self.nonnormalset, self.otherparentset)

    def getfiltered(self, path, filterfunc):
        return self._tree.getfiltered(path, filterfunc, id(filterfunc))

    @property
    def filefoldmap(self):
        """Returns a dictionary mapping normalized case paths to their
        non-normalized versions.
        """
        filterfunc = util.normcase

        def lookup(path):
            tree = self._tree
            # Treestate returns all matched files. Only return one to be
            # compatible with the old API.
            candidates = tree.getfiltered(path, filterfunc, id(filterfunc))
            for candidate in candidates:
                # Skip untracked or removed files.
                if (self.get(candidate, None) or ("?",))[0] not in "r?":
                    return candidate
            return None

        return _overlaydict(lookup)

    def hastrackeddir(self, d):
        """
        Returns True if the dirstate contains a tracked (not removed) file
        in this directory.
        """
        if not d.endswith("/"):
            d += "/"
        state = self._tree.getdir(d)  # [union, intersection]
        return bool(state and (state[0] & treestate.EXIST_NEXT))

    def hasdir(self, d):
        """
        Returns True if the dirstate contains a file (tracked or removed)
        in this directory.
        """
        if not d.endswith("/"):
            d += "/"
        return self._tree.hasdir(d)

    def parents(self):
        return self._parents

    def p1(self):
        return self._parents[0]

    def p2(self):
        return self._parents[1]

    def setparents(self, p1, p2):
        self._parents = (p1, p2)

        metadata = self.getmetadata()
        metadata.update({"p1": None, "p2": None})
        if self._parents[0] != node.nullid:
            metadata["p1"] = node.hex(self._parents[0])
        if self._parents[1] != node.nullid:
            metadata["p2"] = node.hex(self._parents[1])
        self._tree.setmetadata(_packmetadata(metadata))

    def _parsedirstate(content):
        """Parse given dirstate metadata file"""
        f = io.BytesIO(content)
        p1 = f.read(20) or node.nullid
        p2 = f.read(20) or node.nullid
        header = f.read(len(HEADER))
        if header and header != HEADER:
            raise error.Abort(_("working directory state appears damaged!"))
        # simple key-value serialization
        metadata = _unpackmetadata(f.read())
        if metadata:
            try:
                # main append-only tree state filename and root offset
                filename = metadata["filename"]
                rootid = int(metadata["rootid"])
                # whether to write a new file or not during "write"
                threshold = int(metadata.get("threshold", 0))
            except (KeyError, ValueError):
                raise error.Abort(_("working directory state appears damaged!"))
        else:
            filename = None
            rootid = 0
            threshold = 0

        return p1, p2, filename, rootid, threshold

    def _read(self, tree):
        """Read every metadata automatically"""
        content = b""
        try:
            fp, _mode = txnutil.trypending(self._root, self._vfs, "dirstate")
            with fp:
                content = fp.read()
        except IOError as ex:
            if ex.errno != errno.ENOENT:
                raise
        metadata = _unpackmetadata(tree.getmetadata())

        self._tree = tree

        p1, p2, filename, rootid, threshold = treestatemap._parsedirstate(content)
        if filename is None:
            filename = metadata["filename"]
        if filename != self._tree.filename():
            raise error.Abort(
                _(
                    "dirstate metadata treestate name {:?} "
                    "does not match loaded name {:?}"
                )
                % (filename, self._tree.filename())
            )

        self._parents = (p1, p2)
        self._threshold = threshold
        self._rootid = rootid

        # Double check p1 p2 against metadata stored in the tree. This is
        # redundant but many things depend on "dirstate" file format.
        # The metadata here contains (watchman) "clock" which does not exist
        # in "dirstate".
        if metadata:
            for i, p in [(1, p1), (2, p2)]:
                metavalue = metadata.get("p%s" % i, node.nullhex)
                if metavalue != node.hex(p):
                    raise error.Abort(
                        _(
                            "working directory state appears damaged (metadata mismatch - "
                            "p%s %s != %s)!"
                        )
                        % (i, metavalue, node.hex(p))
                    )

    def _gc(self):
        """Remove unreferenced treestate files"""
        fileinuse = set()
        fileinuse.add(self._tree.filename())
        for name in ["dirstate", "undo.dirstate", "undo.backup.dirstate"]:
            try:
                content = self._vfs.tryread(name)
                _p1, _p2, filename = treestatemap._parsedirstate(content)[:3]
                fileinuse.add(filename)
            except Exception:
                # dirstate file does not exist, or is in an incompatible
                # format.
                pass
        from . import dirstate  # avoid cycle

        fsnow = dirstate._getfsnow(self._vfs)
        maxmtime = fsnow - self._ui.configint("treestate", "mingcage")
        for name in self._vfs.listdir("treestate"):
            if name in fileinuse:
                continue
            if name.endswith(".lock"):
                # .lock files are removed later.
                continue
            try:
                if self._vfs.stat("treestate/%s" % name).st_mtime > maxmtime:
                    continue
            except OSError:
                continue
            self._ui.log("treestate", "removing old unreferenced treestate/%s\n" % name)
            self._ui.debug("removing old unreferenced treestate/%s\n" % name)
            self._vfs.tryunlink("treestate/%s" % name)
        # Remove .lock files if their basename file is removed.
        # .lock files are used on Windows (D43198797).
        filelist = set(self._vfs.listdir("treestate"))
        for name in sorted(filelist):
            if not name.endswith(".lock"):
                continue
            basename = name.split(".", 1)[0]
            if basename not in filelist:
                self._vfs.tryunlink("treestate/%s" % name)

    def write(self, st, now):
        # write .hg/treestate/<uuid>
        self._tree.invalidatemtime(now)

        self._vfs.makedirs("treestate")

        # repack and gc (with wlock acquired by parent functions)
        if self._threshold > 0 and self._rootid > self._threshold:
            # recalculate threshold
            self._threshold = 0
            rootid = self._tree.saveas(self._vfs.join("treestate"))
            self._ui.debug("created treestate/%s\n" % (self._tree.filename(),))
            self._gc()
        else:
            rootid = self._tree.flush()

        # calculate self._threshold
        if self._threshold == 0 and rootid > self._ui.configbytes(
            "treestate", "minrepackthreshold"
        ):
            factor = self._ui.configint("treestate", "repackfactor")
            if factor:
                self._threshold = rootid * factor
            self._ui.debug("treestate repack threshold set to %s\n" % self._threshold)

        # write .hg/dirstate
        st.write(self._parents[0])
        st.write(self._parents[1])
        st.write(HEADER)
        st.write(
            _packmetadata(
                {
                    "filename": self._tree.filename(),
                    "rootid": rootid,
                    "threshold": self._threshold,
                }
            )
        )
        st.close()
        self._rootid = rootid

    @property
    def nonnormalset(self):
        return self.nonnormalsetfiltered(None)

    def nonnormalsetfiltered(self, dirfilter):
        """Calculate nonnormalset with a directory filter applied to unknown
        (untracked, "?") files.

        The directory filter is usually the ignore filter. Since treestate only
        tracks "?" files with fsmonitor, dirfilter makes less sense for
        non-fsmonitor usecases.
        """
        # not normal: hg dirstate state != 'n', or mtime == -1 (NEED_CHECK)
        tree = self._tree
        unknown = tree.walk(
            treestate.NEED_CHECK,
            treestate.EXIST_P1 | treestate.EXIST_P2 | treestate.EXIST_NEXT,
            dirfilter,
        )
        normalneedcheck1 = tree.walk(
            treestate.NEED_CHECK | treestate.EXIST_P1 | treestate.EXIST_NEXT,
            treestate.EXIST_P2,
        )
        normalneedcheck2 = tree.walk(
            treestate.NEED_CHECK | treestate.EXIST_P2 | treestate.EXIST_NEXT,
            treestate.EXIST_P1,
        )
        merged = tree.walk(treestate.EXIST_P1 | treestate.EXIST_P2, 0)
        added = tree.walk(treestate.EXIST_NEXT, treestate.EXIST_P1 | treestate.EXIST_P2)
        removed1 = tree.walk(treestate.EXIST_P1, treestate.EXIST_NEXT)
        removed2 = tree.walk(treestate.EXIST_P2, treestate.EXIST_NEXT)
        return set(
            unknown
            + normalneedcheck1
            + normalneedcheck2
            + merged
            + added
            + removed1
            + removed2
        )

    @property
    def otherparentset(self):
        # Only exist in P2
        return set(self._tree.walk(treestate.EXIST_P2, treestate.EXIST_P1))

    @property
    def identity(self):
        return "%s-%s" % (self._tree.filename(), self._rootid)

    @property
    def dirfoldmap(self):
        filterfunc = util.normcase

        def lookup(path):
            tree = self._tree
            candidates = tree.getfiltered(path + "/", filterfunc, id(filterfunc))
            for candidate in candidates:
                # The mapped directory should have at least one tracked file.
                if self.hastrackeddir(candidate):
                    return candidate.rstrip("/")
            return None

        return _overlaydict(lookup)

    def copy(self, source, dest):
        if source == dest:
            return
        existing = self._get(dest)
        if existing:
            state, mode, size, mtime, copied = existing
            if copied != source:
                self._tree.insert(dest, state, mode, size, mtime, source)
        else:
            raise error.ProgrammingError("copy dest %r does not exist" % dest)

    # treestate specific methods

    def getmetadata(self):
        return _unpackmetadata(self._tree.getmetadata())

    def updatemetadata(self, items):
        metadata = _unpackmetadata(self._tree.getmetadata())
        metadata.update(items)
        self._tree.setmetadata(_packmetadata(metadata))

    @property
    def _clock(self):
        return self.getmetadata().get("clock") or None

    def needcheck(self, path):
        """Mark a file as NEED_CHECK, so it will be included by 'nonnormalset'

        Return True if the file was changed, False if it's already marked.
        """
        existing = self._get(path)
        if not existing:
            # The file was not in dirstate (untracked). Add it.
            state = treestate.NEED_CHECK
            mode = 0o666
            size = -1
            mtime = -1
            copied = None
        else:
            state, mode, size, mtime, copied = existing
            if treestate.NEED_CHECK & state:
                return False
            state |= treestate.NEED_CHECK
        self._tree.insert(path, state, mode, size, mtime, copied)
        return True

    def clearneedcheck(self, path):
        """Mark a file as not NEED_CHECK, might remove it from 'nonnormalset'

        Return True if the file was changed, False if the file does not have
        NEED_CHECK.
        """
        existing = self._get(path)
        if existing:
            state, mode, size, mtime, copied = existing
            if treestate.NEED_CHECK & state:
                state ^= treestate.NEED_CHECK
                self._tree.insert(path, state, mode, size, mtime, copied)
                return True
        return False

    def copysource(self, path):
        """Return the copysource for path. Return None if it's not copied, or
        path does not exist.
        """
        existing = self._get(path)
        if existing:
            _state, _mode, _size, _mtime, copied = existing
            return copied
        else:
            return None

    def _get(self, path, default=None):
        return self._tree.get(path, default)


def cleanup(ui, repo):
    """Clean up old tree files."""
    repo.dirstate._map._gc()


def repack(ui, repo):
    if "eden" in repo.requirements:
        return
    with repo.wlock(), repo.lock(), repo.transaction("dirstate") as tr:
        repo.dirstate._map._threshold = 1
        repo.dirstate._dirty = True
        repo.dirstate.write(tr)


def reprflags(flags):
    """Turn flags into human-readable string"""
    return " ".join(
        name
        for name in ("EXIST_P1", "EXIST_P2", "EXIST_NEXT", "COPIED", "NEED_CHECK")
        if flags & getattr(treestate, name)
    )
