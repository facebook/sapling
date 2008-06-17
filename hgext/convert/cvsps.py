#
# Mercurial built-in replacement for cvsps.
#
# Copyright 2008, Frank Kingswood <frank@kingswood-consulting.co.uk>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os
import re
import sys
import cPickle as pickle
from mercurial import util
from mercurial.i18n import _

def listsort(list, key):
    "helper to sort by key in Python 2.3"
    try:
        list.sort(key=key)
    except TypeError:
        list.sort(lambda l, r: cmp(key(l), key(r)))

class logentry(object):
    '''Class logentry has the following attributes:
        .author    - author name as CVS knows it
        .branch    - name of branch this revision is on
        .branches  - revision tuple of branches starting at this revision
        .comment   - commit message
        .date      - the commit date as a (time, tz) tuple
        .dead      - true if file revision is dead
        .file      - Name of file
        .lines     - a tuple (+lines, -lines) or None
        .parent    - Previous revision of this entry
        .rcs       - name of file as returned from CVS
        .revision  - revision number as tuple
        .tags      - list of tags on the file
    '''
    def __init__(self, **entries):
        self.__dict__.update(entries)

class logerror(Exception):
    pass

def createlog(ui, directory=None, root="", rlog=True, cache=None):
    '''Collect the CVS rlog'''

    # Because we store many duplicate commit log messages, reusing strings
    # saves a lot of memory and pickle storage space.
    _scache = {}
    def scache(s):
        "return a shared version of a string"
        return _scache.setdefault(s, s)

    ui.status(_('collecting CVS rlog\n'))

    log = []      # list of logentry objects containing the CVS state

    # patterns to match in CVS (r)log output, by state of use
    re_00 = re.compile('RCS file: (.+)$')
    re_01 = re.compile('cvs \\[r?log aborted\\]: (.+)$')
    re_02 = re.compile('cvs (r?log|server): (.+)\n$')
    re_03 = re.compile("(Cannot access.+CVSROOT)|(can't create temporary directory.+)$")
    re_10 = re.compile('Working file: (.+)$')
    re_20 = re.compile('symbolic names:')
    re_30 = re.compile('\t(.+): ([\\d.]+)$')
    re_31 = re.compile('----------------------------$')
    re_32 = re.compile('=============================================================================$')
    re_50 = re.compile('revision ([\\d.]+)(\s+locked by:\s+.+;)?$')
    re_60 = re.compile(r'date:\s+(.+);\s+author:\s+(.+);\s+state:\s+(.+?);(\s+lines:\s+(\+\d+)?\s+(-\d+)?;)?')
    re_70 = re.compile('branches: (.+);$')

    prefix = ''   # leading path to strip of what we get from CVS

    if directory is None:
        # Current working directory

        # Get the real directory in the repository
        try:
            prefix = file(os.path.join('CVS','Repository')).read().strip()
            if prefix == ".":
                prefix = ""
            directory = prefix
        except IOError:
            raise logerror('Not a CVS sandbox')

        if prefix and not prefix.endswith('/'):
            prefix += '/'

        # Use the Root file in the sandbox, if it exists
        try:
            root = file(os.path.join('CVS','Root')).read().strip()
        except IOError:
            pass

    if not root:
        root = os.environ.get('CVSROOT', '')

    # read log cache if one exists
    oldlog = []
    date = None

    if cache:
        cachedir = os.path.expanduser('~/.hg.cvsps')
        if not os.path.exists(cachedir):
            os.mkdir(cachedir)

        # The cvsps cache pickle needs a uniquified name, based on the
        # repository location. The address may have all sort of nasties
        # in it, slashes, colons and such. So here we take just the
        # alphanumerics, concatenated in a way that does not mix up the
        # various components, so that 
        #    :pserver:user@server:/path
        # and
        #    /pserver/user/server/path
        # are mapped to different cache file names.
        cachefile = root.split(":") + [directory, "cache"]
        cachefile = ['-'.join(re.findall(r'\w+', s)) for s in cachefile if s]
        cachefile = os.path.join(cachedir,
                                 '.'.join([s for s in cachefile if s]))

    if cache == 'update':
        try:
            ui.note(_('reading cvs log cache %s\n') % cachefile)
            oldlog = pickle.load(file(cachefile))
            ui.note(_('cache has %d log entries\n') % len(oldlog))
        except Exception, e:
            ui.note(_('error reading cache: %r\n') % e)

        if oldlog:
            date = oldlog[-1].date    # last commit date as a (time,tz) tuple
            date = util.datestr(date, '%Y/%m/%d %H:%M:%S %1%2')

    # build the CVS commandline
    cmd = ['cvs', '-q']
    if root:
        cmd.append('-d%s' % root)
        p = root.split(':')[-1]
        if not p.endswith('/'):
            p += '/'
        prefix = p + prefix
    cmd.append(['log', 'rlog'][rlog])
    if date:
        # no space between option and date string
        cmd.append('-d>%s' % date)
    cmd.append(directory)

    # state machine begins here
    tags = {}     # dictionary of revisions on current file with their tags
    state = 0
    store = False # set when a new record can be appended

    cmd = [util.shellquote(arg) for arg in cmd]
    ui.note("running %s\n" % (' '.join(cmd)))
    ui.debug("prefix=%r directory=%r root=%r\n" % (prefix, directory, root))

    for line in util.popen(' '.join(cmd)):
        if line.endswith('\n'):
            line = line[:-1]
        #ui.debug('state=%d line=%r\n' % (state, line))

        if state == 0:
            # initial state, consume input until we see 'RCS file'
            match = re_00.match(line)
            if match:
                rcs = match.group(1)
                tags = {}
                if rlog:
                    filename = rcs[:-2]
                    if filename.startswith(prefix):
                        filename = filename[len(prefix):]
                    if filename.startswith('/'):
                        filename = filename[1:]
                    if filename.startswith('Attic/'):
                        filename = filename[6:]
                    else:
                        filename = filename.replace('/Attic/', '/')
                    state = 2
                    continue
                state = 1
                continue
            match = re_01.match(line)
            if match:
                raise Exception(match.group(1))
            match = re_02.match(line)
            if match:
                raise Exception(match.group(2))
            if re_03.match(line):
                raise Exception(line)

        elif state == 1:
            # expect 'Working file' (only when using log instead of rlog)
            match = re_10.match(line)
            assert match, _('RCS file must be followed by working file')
            filename = match.group(1)
            state = 2

        elif state == 2:
            # expect 'symbolic names'
            if re_20.match(line):
                state = 3

        elif state == 3:
            # read the symbolic names and store as tags
            match = re_30.match(line)
            if match:
                rev = [int(x) for x in match.group(2).split('.')]

                # Convert magic branch number to an odd-numbered one
                revn = len(rev)
                if revn > 3 and (revn % 2) == 0 and rev[-2] == 0:
                    rev = rev[:-2] + rev[-1:]
                rev = tuple(rev)

                if rev not in tags:
                    tags[rev] = []
                tags[rev].append(match.group(1))

            elif re_31.match(line):
                state = 5
            elif re_32.match(line):
                state = 0

        elif state == 4:
            # expecting '------' separator before first revision
            if re_31.match(line):
                state = 5
            else:
                assert not re_32.match(line), _('Must have at least some revisions')

        elif state == 5:
            # expecting revision number and possibly (ignored) lock indication
            # we create the logentry here from values stored in states 0 to 4,
            # as this state is re-entered for subsequent revisions of a file.
            match = re_50.match(line)
            assert match, _('expected revision number')
            e = logentry(rcs=scache(rcs), file=scache(filename),
                    revision=tuple([int(x) for x in match.group(1).split('.')]),
                    branches=[], parent=None)
            state = 6

        elif state == 6:
            # expecting date, author, state, lines changed
            match = re_60.match(line)
            assert match, _('revision must be followed by date line')
            d = match.group(1)
            if d[2] == '/':
                # Y2K
                d = '19' + d

            if len(d.split()) != 3:
                # cvs log dates always in GMT
                d = d + ' UTC'
            e.date = util.parsedate(d, ['%y/%m/%d %H:%M:%S', '%Y/%m/%d %H:%M:%S', '%Y-%m-%d %H:%M:%S'])
            e.author = scache(match.group(2))
            e.dead = match.group(3).lower() == 'dead'

            if match.group(5):
                if match.group(6):
                    e.lines = (int(match.group(5)), int(match.group(6)))
                else:
                    e.lines = (int(match.group(5)), 0)
            elif match.group(6):
                e.lines = (0, int(match.group(6)))
            else:
                e.lines = None
            e.comment = []
            state = 7

        elif state == 7:
            # read the revision numbers of branches that start at this revision
            # or store the commit log message otherwise
            m = re_70.match(line)
            if m:
                e.branches = [tuple([int(y) for y in x.strip().split('.')])
                                for x in m.group(1).split(';')]
                state = 8
            elif re_31.match(line):
                state = 5
                store = True
            elif re_32.match(line):
                state = 0
                store = True
            else:
                e.comment.append(line)

        elif state == 8:
            # store commit log message
            if re_31.match(line):
                state = 5
                store = True
            elif re_32.match(line):
                state = 0
                store = True
            else:
                e.comment.append(line)

        if store:
            # clean up the results and save in the log.
            store = False
            e.tags = [scache(x) for x in tags.get(e.revision, [])]
            e.tags.sort()
            e.comment = scache('\n'.join(e.comment))

            revn = len(e.revision)
            if revn > 3 and (revn % 2) == 0:
                e.branch = tags.get(e.revision[:-1], [None])[0]
            else:
                e.branch = None

            log.append(e)

            if len(log) % 100 == 0:
                ui.status(util.ellipsis('%d %s' % (len(log), e.file), 80)+'\n')

    listsort(log, key=lambda x:(x.rcs, x.revision))

    # find parent revisions of individual files
    versions = {}
    for e in log:
        branch = e.revision[:-1]
        p = versions.get((e.rcs, branch), None)
        if p is None:
            p = e.revision[:-2]
        e.parent = p
        versions[(e.rcs, branch)] = e.revision

    # update the log cache
    if cache:
        if log:
            # join up the old and new logs
            listsort(log, key=lambda x:x.date)

            if oldlog and oldlog[-1].date >= log[0].date:
                raise logerror('Log cache overlaps with new log entries,'
                               ' re-run without cache.')

            log = oldlog + log

            # write the new cachefile
            ui.note(_('writing cvs log cache %s\n') % cachefile)
            pickle.dump(log, file(cachefile, 'w'))
        else:
            log = oldlog

    ui.status(_('%d log entries\n') % len(log))

    return log


class changeset(object):
    '''Class changeset has the following attributes:
        .author    - author name as CVS knows it
        .branch    - name of branch this changeset is on, or None
        .comment   - commit message
        .date      - the commit date as a (time,tz) tuple
        .entries   - list of logentry objects in this changeset
        .parents   - list of one or two parent changesets
        .tags      - list of tags on this changeset
    '''
    def __init__(self, **entries):
        self.__dict__.update(entries)

def createchangeset(ui, log, fuzz=60, mergefrom=None, mergeto=None):
    '''Convert log into changesets.'''

    ui.status(_('creating changesets\n'))

    # Merge changesets

    listsort(log, key=lambda x:(x.comment, x.author, x.branch, x.date))

    changesets = []
    files = {}
    c = None
    for i, e in enumerate(log):

        # Check if log entry belongs to the current changeset or not.
        if not (c and
                  e.comment == c.comment and
                  e.author == c.author and
                  e.branch == c.branch and
                  ((c.date[0] + c.date[1]) <=
                   (e.date[0] + e.date[1]) <=
                   (c.date[0] + c.date[1]) + fuzz) and
                  e.file not in files):
            c = changeset(comment=e.comment, author=e.author,
                          branch=e.branch, date=e.date, entries=[])
            changesets.append(c)
            files = {}
            if len(changesets) % 100 == 0:
                t = '%d %s' % (len(changesets), repr(e.comment)[1:-1])
                ui.status(util.ellipsis(t, 80) + '\n')

        e.Changeset = c
        c.entries.append(e)
        files[e.file] = True
        c.date = e.date       # changeset date is date of latest commit in it

    # Sort files in each changeset

    for c in changesets:
        def pathcompare(l, r):
            'Mimic cvsps sorting order'
            l = l.split('/')
            r = r.split('/')
            nl = len(l)
            nr = len(r)
            n = min(nl, nr)
            for i in range(n):
                if i + 1 == nl and nl < nr:
                    return -1
                elif i + 1 == nr and nl > nr:
                    return +1
                elif l[i] < r[i]:
                    return -1
                elif l[i] > r[i]:
                    return +1
            return 0
        def entitycompare(l, r):
            return pathcompare(l.file, r.file)

        c.entries.sort(entitycompare)

    # Sort changesets by date

    def cscmp(l, r):
        d = sum(l.date) - sum(r.date)
        if d:
            return d

        # detect vendor branches and initial commits on a branch
        le = {}
        for e in l.entries:
            le[e.rcs] = e.revision
        re = {}
        for e in r.entries:
            re[e.rcs] = e.revision

        d = 0
        for e in l.entries:
            if re.get(e.rcs, None) == e.parent:
                assert not d
                d = 1
                break

        for e in r.entries:
            if le.get(e.rcs, None) == e.parent:
                assert not d
                d = -1
                break

        return d

    changesets.sort(cscmp)

    # Collect tags

    globaltags = {}
    for c in changesets:
        tags = {}
        for e in c.entries:
            for tag in e.tags:
                # remember which is the latest changeset to have this tag
                globaltags[tag] = c

    for c in changesets:
        tags = {}
        for e in c.entries:
            for tag in e.tags:
                tags[tag] = True
        # remember tags only if this is the latest changeset to have it
        tagnames = [tag for tag in tags if globaltags[tag] is c]
        tagnames.sort()
        c.tags = tagnames

    # Find parent changesets, handle {{mergetobranch BRANCHNAME}}
    # by inserting dummy changesets with two parents, and handle
    # {{mergefrombranch BRANCHNAME}} by setting two parents.

    if mergeto is None:
        mergeto = r'{{mergetobranch ([-\w]+)}}'
    if mergeto:
        mergeto = re.compile(mergeto)

    if mergefrom is None:
        mergefrom = r'{{mergefrombranch ([-\w]+)}}'
    if mergefrom:
        mergefrom = re.compile(mergefrom)

    versions = {}    # changeset index where we saw any particular file version
    branches = {}    # changeset index where we saw a branch
    n = len(changesets)
    i = 0
    while i<n:
        c = changesets[i]

        for f in c.entries:
            versions[(f.rcs, f.revision)] = i

        p = None
        if c.branch in branches:
            p = branches[c.branch]
        else:
            for f in c.entries:
                p = max(p, versions.get((f.rcs, f.parent), None))

        c.parents = []
        if p is not None:
            c.parents.append(changesets[p])

        if mergefrom:
            m = mergefrom.search(c.comment)
            if m:
                m = m.group(1)
                if m == 'HEAD':
                    m = None
                if m in branches and c.branch != m:
                    c.parents.append(changesets[branches[m]])

        if mergeto:
            m = mergeto.search(c.comment)
            if m:
                try:
                    m = m.group(1)
                    if m == 'HEAD':
                        m = None
                except:
                    m = None   # if no group found then merge to HEAD
                if m in branches and c.branch != m:
                    # insert empty changeset for merge
                    cc = changeset(author=c.author, branch=m, date=c.date,
                            comment='convert-repo: CVS merge from branch %s' % c.branch,
                            entries=[], tags=[], parents=[changesets[branches[m]], c])
                    changesets.insert(i + 1, cc)
                    branches[m] = i + 1

                    # adjust our loop counters now we have inserted a new entry
                    n += 1
                    i += 2
                    continue

        branches[c.branch] = i
        i += 1

    # Number changesets

    for i, c in enumerate(changesets):
        c.id = i + 1

    ui.status(_('%d changeset entries\n') % len(changesets))

    return changesets
