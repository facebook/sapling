# phabricator.py - simple Phabricator integration
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""simple Phabricator integration

This extension provides a ``phabsend`` command which sends a stack of
changesets to Phabricator, and a ``phabread`` command which prints a stack of
revisions in a format suitable for :hg:`import`, and a ``phabupdate`` command
to update statuses in batch.

By default, Phabricator requires ``Test Plan`` which might prevent some
changeset from being sent. The requirement could be disabled by changing
``differential.require-test-plan-field`` config server side.

Config::

    [phabricator]
    # Phabricator URL
    url = https://phab.example.com/

    # API token. Get it from https://$HOST/conduit/login/
    token = cli-xxxxxxxxxxxxxxxxxxxxxxxxxxxx

    # Repo callsign. If a repo has a URL https://$HOST/diffusion/FOO, then its
    # callsign is "FOO".
    callsign = FOO

    # curl command to use. If not set (default), use builtin HTTP library to
    # communicate. If set, use the specified curl command. This could be useful
    # if you need to specify advanced options that is not easily supported by
    # the internal library.
    curlcmd = curl --connect-timeout 2 --retry 3 --silent
"""

from __future__ import absolute_import

import itertools
import json
import operator
import re

from mercurial.node import bin, nullid
from mercurial.i18n import _
from mercurial import (
    cmdutil,
    context,
    encoding,
    error,
    mdiff,
    obsutil,
    parser,
    patch,
    registrar,
    scmutil,
    smartset,
    tags,
    url as urlmod,
    util,
)

cmdtable = {}
command = registrar.command(cmdtable)

colortable = {
    'phabricator.action.created': 'green',
    'phabricator.action.skipped': 'magenta',
    'phabricator.action.updated': 'magenta',
    'phabricator.desc': '',
    'phabricator.drev': 'bold',
    'phabricator.node': '',
}

def urlencodenested(params):
    """like urlencode, but works with nested parameters.

    For example, if params is {'a': ['b', 'c'], 'd': {'e': 'f'}}, it will be
    flattened to {'a[0]': 'b', 'a[1]': 'c', 'd[e]': 'f'} and then passed to
    urlencode. Note: the encoding is consistent with PHP's http_build_query.
    """
    flatparams = util.sortdict()
    def process(prefix, obj):
        items = {list: enumerate, dict: lambda x: x.items()}.get(type(obj))
        if items is None:
            flatparams[prefix] = obj
        else:
            for k, v in items(obj):
                if prefix:
                    process('%s[%s]' % (prefix, k), v)
                else:
                    process(k, v)
    process('', params)
    return util.urlreq.urlencode(flatparams)

def readurltoken(repo):
    """return conduit url, token and make sure they exist

    Currently read from [phabricator] config section. In the future, it might
    make sense to read from .arcconfig and .arcrc as well.
    """
    values = []
    section = 'phabricator'
    for name in ['url', 'token']:
        value = repo.ui.config(section, name)
        if not value:
            raise error.Abort(_('config %s.%s is required') % (section, name))
        values.append(value)
    return values

def callconduit(repo, name, params):
    """call Conduit API, params is a dict. return json.loads result, or None"""
    host, token = readurltoken(repo)
    url, authinfo = util.url('/'.join([host, 'api', name])).authinfo()
    repo.ui.debug('Conduit Call: %s %s\n' % (url, params))
    params = params.copy()
    params['api.token'] = token
    data = urlencodenested(params)
    curlcmd = repo.ui.config('phabricator', 'curlcmd')
    if curlcmd:
        sin, sout = util.popen2('%s -d @- %s' % (curlcmd, util.shellquote(url)))
        sin.write(data)
        sin.close()
        body = sout.read()
    else:
        urlopener = urlmod.opener(repo.ui, authinfo)
        request = util.urlreq.request(url, data=data)
        body = urlopener.open(request).read()
    repo.ui.debug('Conduit Response: %s\n' % body)
    parsed = json.loads(body)
    if parsed.get(r'error_code'):
        msg = (_('Conduit Error (%s): %s')
               % (parsed[r'error_code'], parsed[r'error_info']))
        raise error.Abort(msg)
    return parsed[r'result']

@command('debugcallconduit', [], _('METHOD'))
def debugcallconduit(ui, repo, name):
    """call Conduit API

    Call parameters are read from stdin as a JSON blob. Result will be written
    to stdout as a JSON blob.
    """
    params = json.loads(ui.fin.read())
    result = callconduit(repo, name, params)
    s = json.dumps(result, sort_keys=True, indent=2, separators=(',', ': '))
    ui.write('%s\n' % s)

def getrepophid(repo):
    """given callsign, return repository PHID or None"""
    # developer config: phabricator.repophid
    repophid = repo.ui.config('phabricator', 'repophid')
    if repophid:
        return repophid
    callsign = repo.ui.config('phabricator', 'callsign')
    if not callsign:
        return None
    query = callconduit(repo, 'diffusion.repository.search',
                        {'constraints': {'callsigns': [callsign]}})
    if len(query[r'data']) == 0:
        return None
    repophid = encoding.strtolocal(query[r'data'][0][r'phid'])
    repo.ui.setconfig('phabricator', 'repophid', repophid)
    return repophid

_differentialrevisiontagre = re.compile('\AD([1-9][0-9]*)\Z')
_differentialrevisiondescre = re.compile(
    '^Differential Revision:\s*(?:.*)D([1-9][0-9]*)$', re.M)

def getoldnodedrevmap(repo, nodelist):
    """find previous nodes that has been sent to Phabricator

    return {node: (oldnode, Differential diff, Differential Revision ID)}
    for node in nodelist with known previous sent versions, or associated
    Differential Revision IDs. ``oldnode`` and ``Differential diff`` could
    be ``None``.

    Examines commit messages like "Differential Revision:" to get the
    association information.

    If such commit message line is not found, examines all precursors and their
    tags. Tags with format like "D1234" are considered a match and the node
    with that tag, and the number after "D" (ex. 1234) will be returned.

    The ``old node``, if not None, is guaranteed to be the last diff of
    corresponding Differential Revision, and exist in the repo.
    """
    url, token = readurltoken(repo)
    unfi = repo.unfiltered()
    nodemap = unfi.changelog.nodemap

    result = {} # {node: (oldnode?, lastdiff?, drev)}
    toconfirm = {} # {node: (force, {precnode}, drev)}
    for node in nodelist:
        ctx = unfi[node]
        # For tags like "D123", put them into "toconfirm" to verify later
        precnodes = list(obsutil.allpredecessors(unfi.obsstore, [node]))
        for n in precnodes:
            if n in nodemap:
                for tag in unfi.nodetags(n):
                    m = _differentialrevisiontagre.match(tag)
                    if m:
                        toconfirm[node] = (0, set(precnodes), int(m.group(1)))
                        continue

        # Check commit message
        m = _differentialrevisiondescre.search(ctx.description())
        if m:
            toconfirm[node] = (1, set(precnodes), int(m.group(1)))

    # Double check if tags are genuine by collecting all old nodes from
    # Phabricator, and expect precursors overlap with it.
    if toconfirm:
        drevs = [drev for force, precs, drev in toconfirm.values()]
        alldiffs = callconduit(unfi, 'differential.querydiffs',
                               {'revisionIDs': drevs})
        getnode = lambda d: bin(encoding.unitolocal(
            getdiffmeta(d).get(r'node', ''))) or None
        for newnode, (force, precset, drev) in toconfirm.items():
            diffs = [d for d in alldiffs.values()
                     if int(d[r'revisionID']) == drev]

            # "precursors" as known by Phabricator
            phprecset = set(getnode(d) for d in diffs)

            # Ignore if precursors (Phabricator and local repo) do not overlap,
            # and force is not set (when commit message says nothing)
            if not force and not bool(phprecset & precset):
                tagname = 'D%d' % drev
                tags.tag(repo, tagname, nullid, message=None, user=None,
                         date=None, local=True)
                unfi.ui.warn(_('D%s: local tag removed - does not match '
                               'Differential history\n') % drev)
                continue

            # Find the last node using Phabricator metadata, and make sure it
            # exists in the repo
            oldnode = lastdiff = None
            if diffs:
                lastdiff = max(diffs, key=lambda d: int(d[r'id']))
                oldnode = getnode(lastdiff)
                if oldnode and oldnode not in nodemap:
                    oldnode = None

            result[newnode] = (oldnode, lastdiff, drev)

    return result

def getdiff(ctx, diffopts):
    """plain-text diff without header (user, commit message, etc)"""
    output = util.stringio()
    for chunk, _label in patch.diffui(ctx.repo(), ctx.p1().node(), ctx.node(),
                                      None, opts=diffopts):
        output.write(chunk)
    return output.getvalue()

def creatediff(ctx):
    """create a Differential Diff"""
    repo = ctx.repo()
    repophid = getrepophid(repo)
    # Create a "Differential Diff" via "differential.createrawdiff" API
    params = {'diff': getdiff(ctx, mdiff.diffopts(git=True, context=32767))}
    if repophid:
        params['repositoryPHID'] = repophid
    diff = callconduit(repo, 'differential.createrawdiff', params)
    if not diff:
        raise error.Abort(_('cannot create diff for %s') % ctx)
    return diff

def writediffproperties(ctx, diff):
    """write metadata to diff so patches could be applied losslessly"""
    params = {
        'diff_id': diff[r'id'],
        'name': 'hg:meta',
        'data': json.dumps({
            'user': ctx.user(),
            'date': '%d %d' % ctx.date(),
            'node': ctx.hex(),
            'parent': ctx.p1().hex(),
        }),
    }
    callconduit(ctx.repo(), 'differential.setdiffproperty', params)

def createdifferentialrevision(ctx, revid=None, parentrevid=None, oldnode=None,
                               olddiff=None, actions=None):
    """create or update a Differential Revision

    If revid is None, create a new Differential Revision, otherwise update
    revid. If parentrevid is not None, set it as a dependency.

    If oldnode is not None, check if the patch content (without commit message
    and metadata) has changed before creating another diff.

    If actions is not None, they will be appended to the transaction.
    """
    repo = ctx.repo()
    if oldnode:
        diffopts = mdiff.diffopts(git=True, context=32767)
        oldctx = repo.unfiltered()[oldnode]
        neednewdiff = (getdiff(ctx, diffopts) != getdiff(oldctx, diffopts))
    else:
        neednewdiff = True

    transactions = []
    if neednewdiff:
        diff = creatediff(ctx)
        transactions.append({'type': 'update', 'value': diff[r'phid']})
    else:
        # Even if we don't need to upload a new diff because the patch content
        # does not change. We might still need to update its metadata so
        # pushers could know the correct node metadata.
        assert olddiff
        diff = olddiff
    writediffproperties(ctx, diff)

    # Use a temporary summary to set dependency. There might be better ways but
    # I cannot find them for now. But do not do that if we are updating an
    # existing revision (revid is not None) since that introduces visible
    # churns (someone edited "Summary" twice) on the web page.
    if parentrevid and revid is None:
        summary = 'Depends on D%s' % parentrevid
        transactions += [{'type': 'summary', 'value': summary},
                         {'type': 'summary', 'value': ' '}]

    if actions:
        transactions += actions

    # Parse commit message and update related fields.
    desc = ctx.description()
    info = callconduit(repo, 'differential.parsecommitmessage',
                       {'corpus': desc})
    for k, v in info[r'fields'].items():
        if k in ['title', 'summary', 'testPlan']:
            transactions.append({'type': k, 'value': v})

    params = {'transactions': transactions}
    if revid is not None:
        # Update an existing Differential Revision
        params['objectIdentifier'] = revid

    revision = callconduit(repo, 'differential.revision.edit', params)
    if not revision:
        raise error.Abort(_('cannot create revision for %s') % ctx)

    return revision, diff

def userphids(repo, names):
    """convert user names to PHIDs"""
    query = {'constraints': {'usernames': names}}
    result = callconduit(repo, 'user.search', query)
    # username not found is not an error of the API. So check if we have missed
    # some names here.
    data = result[r'data']
    resolved = set(entry[r'fields'][r'username'] for entry in data)
    unresolved = set(names) - resolved
    if unresolved:
        raise error.Abort(_('unknown username: %s')
                          % ' '.join(sorted(unresolved)))
    return [entry[r'phid'] for entry in data]

@command('phabsend',
         [('r', 'rev', [], _('revisions to send'), _('REV')),
          ('', 'amend', True, _('update commit messages')),
          ('', 'reviewer', [], _('specify reviewers')),
          ('', 'confirm', None, _('ask for confirmation before sending'))],
         _('REV [OPTIONS]'))
def phabsend(ui, repo, *revs, **opts):
    """upload changesets to Phabricator

    If there are multiple revisions specified, they will be send as a stack
    with a linear dependencies relationship using the order specified by the
    revset.

    For the first time uploading changesets, local tags will be created to
    maintain the association. After the first time, phabsend will check
    obsstore and tags information so it can figure out whether to update an
    existing Differential Revision, or create a new one.

    If --amend is set, update commit messages so they have the
    ``Differential Revision`` URL, remove related tags. This is similar to what
    arcanist will do, and is more desired in author-push workflows. Otherwise,
    use local tags to record the ``Differential Revision`` association.

    The --confirm option lets you confirm changesets before sending them. You
    can also add following to your configuration file to make it default
    behaviour::

        [phabsend]
        confirm = true

    phabsend will check obsstore and the above association to decide whether to
    update an existing Differential Revision, or create a new one.
    """
    revs = list(revs) + opts.get('rev', [])
    revs = scmutil.revrange(repo, revs)

    if not revs:
        raise error.Abort(_('phabsend requires at least one changeset'))
    if opts.get('amend'):
        cmdutil.checkunfinished(repo)

    # {newnode: (oldnode, olddiff, olddrev}
    oldmap = getoldnodedrevmap(repo, [repo[r].node() for r in revs])

    confirm = ui.configbool('phabsend', 'confirm')
    confirm |= bool(opts.get('confirm'))
    if confirm:
        confirmed = _confirmbeforesend(repo, revs, oldmap)
        if not confirmed:
            raise error.Abort(_('phabsend cancelled'))

    actions = []
    reviewers = opts.get('reviewer', [])
    if reviewers:
        phids = userphids(repo, reviewers)
        actions.append({'type': 'reviewers.add', 'value': phids})

    drevids = [] # [int]
    diffmap = {} # {newnode: diff}

    # Send patches one by one so we know their Differential Revision IDs and
    # can provide dependency relationship
    lastrevid = None
    for rev in revs:
        ui.debug('sending rev %d\n' % rev)
        ctx = repo[rev]

        # Get Differential Revision ID
        oldnode, olddiff, revid = oldmap.get(ctx.node(), (None, None, None))
        if oldnode != ctx.node() or opts.get('amend'):
            # Create or update Differential Revision
            revision, diff = createdifferentialrevision(
                ctx, revid, lastrevid, oldnode, olddiff, actions)
            diffmap[ctx.node()] = diff
            newrevid = int(revision[r'object'][r'id'])
            if revid:
                action = 'updated'
            else:
                action = 'created'

            # Create a local tag to note the association, if commit message
            # does not have it already
            m = _differentialrevisiondescre.search(ctx.description())
            if not m or int(m.group(1)) != newrevid:
                tagname = 'D%d' % newrevid
                tags.tag(repo, tagname, ctx.node(), message=None, user=None,
                         date=None, local=True)
        else:
            # Nothing changed. But still set "newrevid" so the next revision
            # could depend on this one.
            newrevid = revid
            action = 'skipped'

        actiondesc = ui.label(
            {'created': _('created'),
             'skipped': _('skipped'),
             'updated': _('updated')}[action],
            'phabricator.action.%s' % action)
        drevdesc = ui.label('D%s' % newrevid, 'phabricator.drev')
        nodedesc = ui.label(bytes(ctx), 'phabricator.node')
        desc = ui.label(ctx.description().split('\n')[0], 'phabricator.desc')
        ui.write(_('%s - %s - %s: %s\n') % (drevdesc, actiondesc, nodedesc,
                                            desc))
        drevids.append(newrevid)
        lastrevid = newrevid

    # Update commit messages and remove tags
    if opts.get('amend'):
        unfi = repo.unfiltered()
        drevs = callconduit(repo, 'differential.query', {'ids': drevids})
        with repo.wlock(), repo.lock(), repo.transaction('phabsend'):
            wnode = unfi['.'].node()
            mapping = {} # {oldnode: [newnode]}
            for i, rev in enumerate(revs):
                old = unfi[rev]
                drevid = drevids[i]
                drev = [d for d in drevs if int(d[r'id']) == drevid][0]
                newdesc = getdescfromdrev(drev)
                # Make sure commit message contain "Differential Revision"
                if old.description() != newdesc:
                    parents = [
                        mapping.get(old.p1().node(), (old.p1(),))[0],
                        mapping.get(old.p2().node(), (old.p2(),))[0],
                    ]
                    new = context.metadataonlyctx(
                        repo, old, parents=parents, text=newdesc,
                        user=old.user(), date=old.date(), extra=old.extra())
                    newnode = new.commit()
                    mapping[old.node()] = [newnode]
                    # Update diff property
                    writediffproperties(unfi[newnode], diffmap[old.node()])
                # Remove local tags since it's no longer necessary
                tagname = 'D%d' % drevid
                if tagname in repo.tags():
                    tags.tag(repo, tagname, nullid, message=None, user=None,
                             date=None, local=True)
            scmutil.cleanupnodes(repo, mapping, 'phabsend')
            if wnode in mapping:
                unfi.setparents(mapping[wnode][0])

# Map from "hg:meta" keys to header understood by "hg import". The order is
# consistent with "hg export" output.
_metanamemap = util.sortdict([(r'user', 'User'), (r'date', 'Date'),
                              (r'node', 'Node ID'), (r'parent', 'Parent ')])

def _confirmbeforesend(repo, revs, oldmap):
    url, token = readurltoken(repo)
    ui = repo.ui
    for rev in revs:
        ctx = repo[rev]
        desc = ctx.description().splitlines()[0]
        oldnode, olddiff, drevid = oldmap.get(ctx.node(), (None, None, None))
        if drevid:
            drevdesc = ui.label('D%s' % drevid, 'phabricator.drev')
        else:
            drevdesc = ui.label(_('NEW'), 'phabricator.drev')

        ui.write(_('%s - %s: %s\n') % (drevdesc,
                                       ui.label(bytes(ctx), 'phabricator.node'),
                                       ui.label(desc, 'phabricator.desc')))

    if ui.promptchoice(_('Send the above changes to %s (yn)?'
                         '$$ &Yes $$ &No') % url):
        return False

    return True

_knownstatusnames = {'accepted', 'needsreview', 'needsrevision', 'closed',
                     'abandoned'}

def _getstatusname(drev):
    """get normalized status name from a Differential Revision"""
    return drev[r'statusName'].replace(' ', '').lower()

# Small language to specify differential revisions. Support symbols: (), :X,
# +, and -.

_elements = {
    # token-type: binding-strength, primary, prefix, infix, suffix
    '(':      (12, None, ('group', 1, ')'), None, None),
    ':':      (8, None, ('ancestors', 8), None, None),
    '&':      (5,  None, None, ('and_', 5), None),
    '+':      (4,  None, None, ('add', 4), None),
    '-':      (4,  None, None, ('sub', 4), None),
    ')':      (0,  None, None, None, None),
    'symbol': (0, 'symbol', None, None, None),
    'end':    (0, None, None, None, None),
}

def _tokenize(text):
    view = memoryview(text) # zero-copy slice
    special = '():+-& '
    pos = 0
    length = len(text)
    while pos < length:
        symbol = ''.join(itertools.takewhile(lambda ch: ch not in special,
                                             view[pos:]))
        if symbol:
            yield ('symbol', symbol, pos)
            pos += len(symbol)
        else: # special char, ignore space
            if text[pos] != ' ':
                yield (text[pos], None, pos)
            pos += 1
    yield ('end', None, pos)

def _parse(text):
    tree, pos = parser.parser(_elements).parse(_tokenize(text))
    if pos != len(text):
        raise error.ParseError('invalid token', pos)
    return tree

def _parsedrev(symbol):
    """str -> int or None, ex. 'D45' -> 45; '12' -> 12; 'x' -> None"""
    if symbol.startswith('D') and symbol[1:].isdigit():
        return int(symbol[1:])
    if symbol.isdigit():
        return int(symbol)

def _prefetchdrevs(tree):
    """return ({single-drev-id}, {ancestor-drev-id}) to prefetch"""
    drevs = set()
    ancestordrevs = set()
    op = tree[0]
    if op == 'symbol':
        r = _parsedrev(tree[1])
        if r:
            drevs.add(r)
    elif op == 'ancestors':
        r, a = _prefetchdrevs(tree[1])
        drevs.update(r)
        ancestordrevs.update(r)
        ancestordrevs.update(a)
    else:
        for t in tree[1:]:
            r, a = _prefetchdrevs(t)
            drevs.update(r)
            ancestordrevs.update(a)
    return drevs, ancestordrevs

def querydrev(repo, spec):
    """return a list of "Differential Revision" dicts

    spec is a string using a simple query language, see docstring in phabread
    for details.

    A "Differential Revision dict" looks like:

        {
            "id": "2",
            "phid": "PHID-DREV-672qvysjcczopag46qty",
            "title": "example",
            "uri": "https://phab.example.com/D2",
            "dateCreated": "1499181406",
            "dateModified": "1499182103",
            "authorPHID": "PHID-USER-tv3ohwc4v4jeu34otlye",
            "status": "0",
            "statusName": "Needs Review",
            "properties": [],
            "branch": null,
            "summary": "",
            "testPlan": "",
            "lineCount": "2",
            "activeDiffPHID": "PHID-DIFF-xoqnjkobbm6k4dk6hi72",
            "diffs": [
              "3",
              "4",
            ],
            "commits": [],
            "reviewers": [],
            "ccs": [],
            "hashes": [],
            "auxiliary": {
              "phabricator:projects": [],
              "phabricator:depends-on": [
                "PHID-DREV-gbapp366kutjebt7agcd"
              ]
            },
            "repositoryPHID": "PHID-REPO-hub2hx62ieuqeheznasv",
            "sourcePath": null
        }
    """
    def fetch(params):
        """params -> single drev or None"""
        key = (params.get(r'ids') or params.get(r'phids') or [None])[0]
        if key in prefetched:
            return prefetched[key]
        drevs = callconduit(repo, 'differential.query', params)
        # Fill prefetched with the result
        for drev in drevs:
            prefetched[drev[r'phid']] = drev
            prefetched[int(drev[r'id'])] = drev
        if key not in prefetched:
            raise error.Abort(_('cannot get Differential Revision %r') % params)
        return prefetched[key]

    def getstack(topdrevids):
        """given a top, get a stack from the bottom, [id] -> [id]"""
        visited = set()
        result = []
        queue = [{r'ids': [i]} for i in topdrevids]
        while queue:
            params = queue.pop()
            drev = fetch(params)
            if drev[r'id'] in visited:
                continue
            visited.add(drev[r'id'])
            result.append(int(drev[r'id']))
            auxiliary = drev.get(r'auxiliary', {})
            depends = auxiliary.get(r'phabricator:depends-on', [])
            for phid in depends:
                queue.append({'phids': [phid]})
        result.reverse()
        return smartset.baseset(result)

    # Initialize prefetch cache
    prefetched = {} # {id or phid: drev}

    tree = _parse(spec)
    drevs, ancestordrevs = _prefetchdrevs(tree)

    # developer config: phabricator.batchsize
    batchsize = repo.ui.configint('phabricator', 'batchsize', 12)

    # Prefetch Differential Revisions in batch
    tofetch = set(drevs)
    for r in ancestordrevs:
        tofetch.update(range(max(1, r - batchsize), r + 1))
    if drevs:
        fetch({r'ids': list(tofetch)})
    validids = sorted(set(getstack(list(ancestordrevs))) | set(drevs))

    # Walk through the tree, return smartsets
    def walk(tree):
        op = tree[0]
        if op == 'symbol':
            drev = _parsedrev(tree[1])
            if drev:
                return smartset.baseset([drev])
            elif tree[1] in _knownstatusnames:
                drevs = [r for r in validids
                         if _getstatusname(prefetched[r]) == tree[1]]
                return smartset.baseset(drevs)
            else:
                raise error.Abort(_('unknown symbol: %s') % tree[1])
        elif op in {'and_', 'add', 'sub'}:
            assert len(tree) == 3
            return getattr(operator, op)(walk(tree[1]), walk(tree[2]))
        elif op == 'group':
            return walk(tree[1])
        elif op == 'ancestors':
            return getstack(walk(tree[1]))
        else:
            raise error.ProgrammingError('illegal tree: %r' % tree)

    return [prefetched[r] for r in walk(tree)]

def getdescfromdrev(drev):
    """get description (commit message) from "Differential Revision"

    This is similar to differential.getcommitmessage API. But we only care
    about limited fields: title, summary, test plan, and URL.
    """
    title = drev[r'title']
    summary = drev[r'summary'].rstrip()
    testplan = drev[r'testPlan'].rstrip()
    if testplan:
        testplan = 'Test Plan:\n%s' % testplan
    uri = 'Differential Revision: %s' % drev[r'uri']
    return '\n\n'.join(filter(None, [title, summary, testplan, uri]))

def getdiffmeta(diff):
    """get commit metadata (date, node, user, p1) from a diff object

    The metadata could be "hg:meta", sent by phabsend, like:

        "properties": {
          "hg:meta": {
            "date": "1499571514 25200",
            "node": "98c08acae292b2faf60a279b4189beb6cff1414d",
            "user": "Foo Bar <foo@example.com>",
            "parent": "6d0abad76b30e4724a37ab8721d630394070fe16"
          }
        }

    Or converted from "local:commits", sent by "arc", like:

        "properties": {
          "local:commits": {
            "98c08acae292b2faf60a279b4189beb6cff1414d": {
              "author": "Foo Bar",
              "time": 1499546314,
              "branch": "default",
              "tag": "",
              "commit": "98c08acae292b2faf60a279b4189beb6cff1414d",
              "rev": "98c08acae292b2faf60a279b4189beb6cff1414d",
              "local": "1000",
              "parents": ["6d0abad76b30e4724a37ab8721d630394070fe16"],
              "summary": "...",
              "message": "...",
              "authorEmail": "foo@example.com"
            }
          }
        }

    Note: metadata extracted from "local:commits" will lose time zone
    information.
    """
    props = diff.get(r'properties') or {}
    meta = props.get(r'hg:meta')
    if not meta and props.get(r'local:commits'):
        commit = sorted(props[r'local:commits'].values())[0]
        meta = {
            r'date': r'%d 0' % commit[r'time'],
            r'node': commit[r'rev'],
            r'user': r'%s <%s>' % (commit[r'author'], commit[r'authorEmail']),
        }
        if len(commit.get(r'parents', ())) >= 1:
            meta[r'parent'] = commit[r'parents'][0]
    return meta or {}

def readpatch(repo, drevs, write):
    """generate plain-text patch readable by 'hg import'

    write is usually ui.write. drevs is what "querydrev" returns, results of
    "differential.query".
    """
    # Prefetch hg:meta property for all diffs
    diffids = sorted(set(max(int(v) for v in drev[r'diffs']) for drev in drevs))
    diffs = callconduit(repo, 'differential.querydiffs', {'ids': diffids})

    # Generate patch for each drev
    for drev in drevs:
        repo.ui.note(_('reading D%s\n') % drev[r'id'])

        diffid = max(int(v) for v in drev[r'diffs'])
        body = callconduit(repo, 'differential.getrawdiff', {'diffID': diffid})
        desc = getdescfromdrev(drev)
        header = '# HG changeset patch\n'

        # Try to preserve metadata from hg:meta property. Write hg patch
        # headers that can be read by the "import" command. See patchheadermap
        # and extract in mercurial/patch.py for supported headers.
        meta = getdiffmeta(diffs[str(diffid)])
        for k in _metanamemap.keys():
            if k in meta:
                header += '# %s %s\n' % (_metanamemap[k], meta[k])

        content = '%s%s\n%s' % (header, desc, body)
        write(encoding.unitolocal(content))

@command('phabread',
         [('', 'stack', False, _('read dependencies'))],
         _('DREVSPEC [OPTIONS]'))
def phabread(ui, repo, spec, **opts):
    """print patches from Phabricator suitable for importing

    DREVSPEC could be a Differential Revision identity, like ``D123``, or just
    the number ``123``. It could also have common operators like ``+``, ``-``,
    ``&``, ``(``, ``)`` for complex queries. Prefix ``:`` could be used to
    select a stack.

    ``abandoned``, ``accepted``, ``closed``, ``needsreview``, ``needsrevision``
    could be used to filter patches by status. For performance reason, they
    only represent a subset of non-status selections and cannot be used alone.

    For example, ``:D6+8-(2+D4)`` selects a stack up to D6, plus D8 and exclude
    D2 and D4. ``:D9 & needsreview`` selects "Needs Review" revisions in a
    stack up to D9.

    If --stack is given, follow dependencies information and read all patches.
    It is equivalent to the ``:`` operator.
    """
    if opts.get('stack'):
        spec = ':(%s)' % spec
    drevs = querydrev(repo, spec)
    readpatch(repo, drevs, ui.write)

@command('phabupdate',
         [('', 'accept', False, _('accept revisions')),
          ('', 'reject', False, _('reject revisions')),
          ('', 'abandon', False, _('abandon revisions')),
          ('', 'reclaim', False, _('reclaim revisions')),
          ('m', 'comment', '', _('comment on the last revision')),
         ], _('DREVSPEC [OPTIONS]'))
def phabupdate(ui, repo, spec, **opts):
    """update Differential Revision in batch

    DREVSPEC selects revisions. See :hg:`help phabread` for its usage.
    """
    flags = [n for n in 'accept reject abandon reclaim'.split() if opts.get(n)]
    if len(flags) > 1:
        raise error.Abort(_('%s cannot be used together') % ', '.join(flags))

    actions = []
    for f in flags:
        actions.append({'type': f, 'value': 'true'})

    drevs = querydrev(repo, spec)
    for i, drev in enumerate(drevs):
        if i + 1 == len(drevs) and opts.get('comment'):
            actions.append({'type': 'comment', 'value': opts['comment']})
        if actions:
            params = {'objectIdentifier': drev[r'phid'],
                      'transactions': actions}
            callconduit(repo, 'differential.revision.edit', params)
