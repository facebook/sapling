# stuff related specifically to patch manipulation / parsing
from mercurial.i18n import _
from mercurial import patch

import cStringIO
import re

lines_re = re.compile(r'@@ -(\d+),(\d+) \+(\d+),(\d+) @@\s*(.*)')

def scanpatch(fp):
    """like patch.iterhunks, but yield different events

    - ('file',    [header_lines + fromfile + tofile])
    - ('context', [context_lines])
    - ('hunk',    [hunk_lines])
    - ('range',   (-start,len, +start,len, diffp))
    """
    lr = patch.linereader(fp)

    def scanwhile(first, p):
        """scan lr while predicate holds"""
        lines = [first]
        while True:
            line = lr.readline()
            if not line:
                break
            if p(line):
                lines.append(line)
            else:
                lr.push(line)
                break
        return lines

    while True:
        line = lr.readline()
        if not line:
            break
        if line.startswith('diff --git a/'):
            def notheader(line):
                s = line.split(None, 1)
                return not s or s[0] not in ('---', 'diff')
            header = scanwhile(line, notheader)
            fromfile = lr.readline()
            if fromfile.startswith('---'):
                tofile = lr.readline()
                header += [fromfile, tofile]
            else:
                lr.push(fromfile)
            yield 'file', header
        elif line[0] == ' ':
            yield 'context', scanwhile(line, lambda l: l[0] in ' \\')
        elif line[0] in '-+':
            yield 'hunk', scanwhile(line, lambda l: l[0] in '-+\\')
        else:
            m = lines_re.match(line)
            if m:
                yield 'range', m.groups()
            else:
                raise patch.PatchError('unknown patch content: %r' % line)

class PatchNode(object):
    """Abstract Class for Patch Graph Nodes
    (i.e. PatchRoot, header, hunk, HunkLine)
    """

    def firstChild(self):
        raise NotImplementedError("method must be implemented by subclass")

    def lastChild(self):
        raise NotImplementedError("method must be implemented by subclass")

    def allChildren(self):
        "Return a list of all of the direct children of this node"
        raise NotImplementedError("method must be implemented by subclass")
    def nextSibling(self):
        """
        Return the closest next item of the same type where there are no items
        of different types between the current item and this closest item.
        If no such item exists, return None.

        """
        raise NotImplementedError("method must be implemented by subclass")

    def prevSibling(self):
        """
        Return the closest previous item of the same type where there are no
        items of different types between the current item and this closest item.
        If no such item exists, return None.

        """
        raise NotImplementedError("method must be implemented by subclass")

    def parentItem(self):
        raise NotImplementedError("method must be implemented by subclass")


    def nextItem(self, constrainLevel=True, skipFolded=True):
        """
        If constrainLevel == True, return the closest next item
        of the same type where there are no items of different types between
        the current item and this closest item.

        If constrainLevel == False, then try to return the next item
        closest to this item, regardless of item's type (header, hunk, or
        HunkLine).

        If skipFolded == True, and the current item is folded, then the child
        items that are hidden due to folding will be skipped when determining
        the next item.

        If it is not possible to get the next item, return None.

        """
        try:
            itemFolded = self.folded
        except AttributeError:
            itemFolded = False
        if constrainLevel:
            return self.nextSibling()
        elif skipFolded and itemFolded:
            nextItem = self.nextSibling()
            if nextItem is None:
                try:
                    nextItem = self.parentItem().nextSibling()
                except AttributeError:
                    nextItem = None
            return nextItem
        else:
            # try child
            item = self.firstChild()
            if item is not None:
                return item

            # else try next sibling
            item = self.nextSibling()
            if item is not None:
                return item

            try:
                # else try parent's next sibling
                item = self.parentItem().nextSibling()
                if item is not None:
                    return item

                # else return grandparent's next sibling (or None)
                return self.parentItem().parentItem().nextSibling()

            except AttributeError: # parent and/or grandparent was None
                return None

    def prevItem(self, constrainLevel=True, skipFolded=True):
        """
        If constrainLevel == True, return the closest previous item
        of the same type where there are no items of different types between
        the current item and this closest item.

        If constrainLevel == False, then try to return the previous item
        closest to this item, regardless of item's type (header, hunk, or
        HunkLine).

        If skipFolded == True, and the current item is folded, then the items
        that are hidden due to folding will be skipped when determining the
        next item.

        If it is not possible to get the previous item, return None.

        """
        if constrainLevel:
            return self.prevSibling()
        else:
            # try previous sibling's last child's last child,
            # else try previous sibling's last child, else try previous sibling
            prevSibling = self.prevSibling()
            if prevSibling is not None:
                prevSiblingLastChild = prevSibling.lastChild()
                if ((prevSiblingLastChild is not None) and
                    not prevSibling.folded):
                    prevSiblingLCLC = prevSiblingLastChild.lastChild()
                    if ((prevSiblingLCLC is not None) and
                        not prevSiblingLastChild.folded):
                        return prevSiblingLCLC
                    else:
                        return prevSiblingLastChild
                else:
                    return prevSibling

            # try parent (or None)
            return self.parentItem()

class Patch(PatchNode, list): # TODO: rename PatchRoot
    """
    List of header objects representing the patch.

    """
    def __init__(self, headerList):
        self.extend(headerList)
        # add parent patch object reference to each header
        for header in self:
            header.patch = self

class header(PatchNode):
    """patch header

    XXX shoudn't we move this to mercurial/patch.py ?
    """
    diff_re = re.compile('diff --git a/(.*) b/(.*)$')
    allhunks_re = re.compile('(?:index|new file|deleted file) ')
    pretty_re = re.compile('(?:new file|deleted file) ')
    special_re = re.compile('(?:index|new|deleted|copy|rename) ')

    def __init__(self, header):
        self.header = header
        self.hunks = []
        # flag to indicate whether to apply this chunk
        self.applied = True
        # flag which only affects the status display indicating if a node's
        # children are partially applied (i.e. some applied, some not).
        self.partial = False

        # flag to indicate whether to display as folded/unfolded to user
        self.folded = True

        # list of all headers in patch
        self.patch = None

        # flag is False if this header was ever unfolded from initial state
        self.neverUnfolded = True
    def binary(self):
        """
        Return True if the file represented by the header is a binary file.
        Otherwise return False.

        """
        for h in self.header:
            if h.startswith('index '):
                return True
        return False

    def pretty(self, fp):
        for h in self.header:
            if h.startswith('index '):
                fp.write(_('this modifies a binary file (all or nothing)\n'))
                break
            if self.pretty_re.match(h):
                fp.write(h)
                if self.binary():
                    fp.write(_('this is a binary file\n'))
                break
            if h.startswith('---'):
                fp.write(_('%d hunks, %d lines changed\n') %
                         (len(self.hunks),
                          sum([h.added + h.removed for h in self.hunks])))
                break
            fp.write(h)

    def prettyStr(self):
        x = cStringIO.StringIO()
        self.pretty(x)
        return x.getvalue()

    def write(self, fp):
        fp.write(''.join(self.header))

    def allhunks(self):
        """
        Return True if the file which the header represents was changed
        completely (i.e.  there is no possibility of applying a hunk of changes
        smaller than the size of the entire file.)  Otherwise return False

        """
        for h in self.header:
            if self.allhunks_re.match(h):
                return True
        return False

    def files(self):
        fromfile, tofile = self.diff_re.match(self.header[0]).groups()
        if fromfile == tofile:
            return [fromfile]
        return [fromfile, tofile]

    def filename(self):
        return self.files()[-1]

    def __repr__(self):
        return '<header %s>' % (' '.join(map(repr, self.files())))

    def special(self):
        for h in self.header:
            if self.special_re.match(h):
                return True

    def nextSibling(self):
        numHeadersInPatch = len(self.patch)
        indexOfThisHeader = self.patch.index(self)

        if indexOfThisHeader < numHeadersInPatch - 1:
            nextHeader = self.patch[indexOfThisHeader + 1]
            return nextHeader
        else:
            return None

    def prevSibling(self):
        indexOfThisHeader = self.patch.index(self)
        if indexOfThisHeader > 0:
            previousHeader = self.patch[indexOfThisHeader - 1]
            return previousHeader
        else:
            return None

    def parentItem(self):
        """
        There is no 'real' parent item of a header that can be selected,
        so return None.
        """
        return None

    def firstChild(self):
        "Return the first child of this item, if one exists.  Otherwise None."
        if len(self.hunks) > 0:
            return self.hunks[0]
        else:
            return None

    def lastChild(self):
        "Return the last child of this item, if one exists.  Otherwise None."
        if len(self.hunks) > 0:
            return self.hunks[-1]
        else:
            return None

    def allChildren(self):
        "Return a list of all of the direct children of this node"
        return self.hunks
class HunkLine(PatchNode):
    "Represents a changed line in a hunk"
    def __init__(self, lineText, hunk):
        self.lineText = lineText
        self.applied = True
        # the parent hunk to which this line belongs
        self.hunk = hunk
        # folding lines currently is not used/needed, but this flag is needed
        # in the prevItem method.
        self.folded = False

    def prettyStr(self):
        return self.lineText

    def nextSibling(self):
        numLinesInHunk = len(self.hunk.changedLines)
        indexOfThisLine = self.hunk.changedLines.index(self)

        if (indexOfThisLine < numLinesInHunk - 1):
            nextLine = self.hunk.changedLines[indexOfThisLine + 1]
            return nextLine
        else:
            return None

    def prevSibling(self):
        indexOfThisLine = self.hunk.changedLines.index(self)
        if indexOfThisLine > 0:
            previousLine = self.hunk.changedLines[indexOfThisLine - 1]
            return previousLine
        else:
            return None

    def parentItem(self):
        "Return the parent to the current item"
        return self.hunk

    def firstChild(self):
        "Return the first child of this item, if one exists.  Otherwise None."
        # hunk-lines don't have children
        return None

    def lastChild(self):
        "Return the last child of this item, if one exists.  Otherwise None."
        # hunk-lines don't have children
        return None

class hunk(PatchNode):
    """patch hunk

    XXX shouldn't we merge this with patch.hunk ?
    """
    maxcontext = 3

    def __init__(self, header, fromline, toline, proc, before, hunk, after):
        def trimcontext(number, lines):
            delta = len(lines) - self.maxcontext
            if False and delta > 0:
                return number + delta, lines[:self.maxcontext]
            return number, lines

        self.header = header
        self.fromline, self.before = trimcontext(fromline, before)
        self.toline, self.after = trimcontext(toline, after)
        self.proc = proc
        self.changedLines = [HunkLine(line, self) for line in hunk]
        self.added, self.removed = self.countchanges()
        # used at end for detecting how many removed lines were un-applied
        self.originalremoved = self.removed

        # flag to indicate whether to display as folded/unfolded to user
        self.folded = True
        # flag to indicate whether to apply this chunk
        self.applied = True
        # flag which only affects the status display indicating if a node's
        # children are partially applied (i.e. some applied, some not).
        self.partial = False

    def nextSibling(self):
        numHunksInHeader = len(self.header.hunks)
        indexOfThisHunk = self.header.hunks.index(self)

        if (indexOfThisHunk < numHunksInHeader - 1):
            nextHunk = self.header.hunks[indexOfThisHunk + 1]
            return nextHunk
        else:
            return None

    def prevSibling(self):
        indexOfThisHunk = self.header.hunks.index(self)
        if indexOfThisHunk > 0:
            previousHunk = self.header.hunks[indexOfThisHunk - 1]
            return previousHunk
        else:
            return None

    def parentItem(self):
        "Return the parent to the current item"
        return self.header

    def firstChild(self):
        "Return the first child of this item, if one exists.  Otherwise None."
        if len(self.changedLines) > 0:
            return self.changedLines[0]
        else:
            return None

    def lastChild(self):
        "Return the last child of this item, if one exists.  Otherwise None."
        if len(self.changedLines) > 0:
            return self.changedLines[-1]
        else:
            return None

    def allChildren(self):
        "Return a list of all of the direct children of this node"
        return self.changedLines
    def countchanges(self):
        """changedLines -> (n+,n-)"""
        add = len([l for l in self.changedLines if l.applied
                   and l.prettyStr()[0] == '+'])
        rem = len([l for l in self.changedLines if l.applied
                   and l.prettyStr()[0] == '-'])
        return add, rem

    def getFromToLine(self):
        # calculate the number of removed lines converted to context lines
        removedConvertedToContext = self.originalremoved - self.removed

        contextLen = (len(self.before) + len(self.after) +
                      removedConvertedToContext)
        if self.after and self.after[-1] == '\\ No newline at end of file\n':
            contextLen -= 1
        fromlen = contextLen + self.removed
        tolen = contextLen + self.added

        # Diffutils manual, section "2.2.2.2 Detailed Description of Unified
        # Format": "An empty hunk is considered to end at the line that
        # precedes the hunk."
        #
        # So, if either of hunks is empty, decrease its line start. --immerrr
        # But only do this if fromline > 0, to avoid having, e.g fromline=-1.
        fromline,toline = self.fromline, self.toline
        if fromline != 0:
            if fromlen == 0:
                fromline -= 1
            if tolen == 0:
                toline -= 1

        fromToLine = '@@ -%d,%d +%d,%d @@%s\n' % (
            fromline, fromlen, toline, tolen,
            self.proc and (' ' + self.proc))
        return fromToLine

    def write(self, fp):
        # updated self.added/removed, which are used by getFromToLine()
        self.added, self.removed = self.countchanges()
        fp.write(self.getFromToLine())

        hunkLineList = []
        # add the following to the list: (1) all applied lines, and
        # (2) all unapplied removal lines (convert these to context lines)
        for changedLine in self.changedLines:
            changedLineStr = changedLine.prettyStr()
            if changedLine.applied:
                hunkLineList.append(changedLineStr)
            elif changedLineStr[0] == "-":
                hunkLineList.append(" " + changedLineStr[1:])

        fp.write(''.join(self.before + hunkLineList + self.after))

    pretty = write

    def filename(self):
        return self.header.filename()

    def prettyStr(self):
        x = cStringIO.StringIO()
        self.pretty(x)
        return x.getvalue()

    def __repr__(self):
        return '<hunk %r@%d>' % (self.filename(), self.fromline)



def parsepatch(changes, fp):
    "Parse a patch, returning a list of header and hunk objects."
    class parser(object):
        """patch parsing state machine"""
        def __init__(self):
            self.fromline = 0
            self.toline = 0
            self.proc = ''
            self.header = None
            self.context = []
            self.before = []
            self.changedlines = []
            self.stream = []
            self.modified, self.added, self.removed = changes

        def _range(self, (fromstart, fromend, tostart, toend, proc)):
            "Store range line info to associated instance variables."
            self.fromline = int(fromstart)
            self.toline = int(tostart)
            self.proc = proc

        def add_new_hunk(self):
            """
            Create a new complete hunk object, adding it to the latest header
            and to self.stream.

            Add all of the previously collected information about
            the hunk to the new hunk object.  This information includes
            header, from/to-lines, function (self.proc), preceding context
            lines, changed lines, as well as the current context lines (which
            follow the changed lines).

            The size of the from/to lines are updated to be correct for the
            next hunk we parse.

            """
            h = hunk(self.header, self.fromline, self.toline, self.proc,
                     self.before, self.changedlines, self.context)
            self.header.hunks.append(h)
            self.stream.append(h)
            self.fromline += len(self.before) + h.removed + len(self.context)
            self.toline += len(self.before) + h.added + len(self.context)
            self.before = []
            self.changedlines = []
            self.context = []
            self.proc = ''

        def _context(self, context):
            """
            Set the value of self.context.

            Also, if an unprocessed set of changelines was previously
            encountered, this is the condition for creating a complete
            hunk object.  In this case, we create and add a new hunk object to
            the most recent header object, and to self.strem. 

            """
            self.context = context
            # if there have been changed lines encountered that haven't yet
            # been add to a hunk.
            if self.changedlines:
                self.add_new_hunk()

        def _changedlines(self, changedlines):
            """
            Store the changed lines in self.changedlines.

            Mark any context lines in the context-line buffer (self.context) as
            lines preceding the changed-lines (i.e. stored in self.before), and
            clear the context-line buffer.

            """
            self.changedlines = changedlines
            self.before = self.context
            self.context = []

        def add_new_header(self, hdr):
            """
            Create a header object containing the header lines, and the
            filename the header applies to.  Add the header to self.stream.

            """
            # if there are any lines in the unchanged-lines buffer, create a 
            # new hunk using them, and add it to the last header.
            if self.changedlines:
                self.add_new_hunk()

            # create a new header and add it to self.stream
            self.header = header(hdr)
            fileName = self.header.filename()
            if fileName in self.modified:
                self.header.changetype = "M"
            elif fileName in self.added:
                self.header.changetype = "A"
            elif fileName in self.removed:
                self.header.changetype = "R"
            self.stream.append(self.header)

        def finished(self):
            # if there are any lines in the unchanged-lines buffer, create a 
            # new hunk using them, and add it to the last header.
            if self.changedlines:
                self.add_new_hunk()

            return self.stream

        transitions = {
            'file': {'context': _context,
                     'file': add_new_header,
                     'hunk': _changedlines,
                     'range': _range},
            'context': {'file': add_new_header,
                        'hunk': _changedlines,
                        'range': _range},
            'hunk': {'context': _context,
                     'file': add_new_header,
                     'range': _range},
            'range': {'context': _context,
                      'hunk': _changedlines},
            }

    p = parser()

    # run the state-machine
    state = 'context'
    for newstate, data in scanpatch(fp):
        try:
            p.transitions[state][newstate](p, data)
        except KeyError:
            raise patch.PatchError('unhandled transition: %s -> %s' %
                                   (state, newstate))
        state = newstate
    return p.finished()

def filterpatch(opts, chunks, chunk_selector, ui):
    """Interactively filter patch chunks into applied-only chunks"""
    chunks = list(chunks)
    # convert chunks list into structure suitable for displaying/modifying
    # with curses.  Create a list of headers only.
    headers = [c for c in chunks if isinstance(c, header)]

    # if there are no changed files
    if len(headers) == 0:
        return []

    # let user choose headers/hunks/lines, and mark their applied flags accordingly
    chunk_selector(opts, headers, ui)

    appliedHunkList = []
    for hdr in headers:
        if (hdr.applied and
            (hdr.special() or len([h for h in hdr.hunks if h.applied]) > 0)):
            appliedHunkList.append(hdr)
            fixoffset = 0
            for hnk in hdr.hunks:
                if hnk.applied:
                    appliedHunkList.append(hnk)
                    # adjust the 'to'-line offset of the hunk to be correct
                    # after de-activating some of the other hunks for this file
                    if fixoffset:
                        #hnk = copy.copy(hnk) # necessary??
                        hnk.toline += fixoffset
                else:
                    fixoffset += hnk.removed - hnk.added

    return appliedHunkList
