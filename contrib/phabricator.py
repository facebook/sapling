# phabricator.py - simple Phabricator integration
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""simple Phabricator integration

This extension provides a ``phabsend`` command which sends a stack of
changesets to Phabricator without amending commit messages, and a ``phabread``
command which prints a stack of revisions in a format suitable
for :hg:`import`.

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

"""

from __future__ import absolute_import

import json
import re

from mercurial.node import bin, nullid
from mercurial.i18n import _
from mercurial import (
    encoding,
    error,
    mdiff,
    obsolete,
    patch,
    registrar,
    scmutil,
    tags,
    url as urlmod,
    util,
)

cmdtable = {}
command = registrar.command(cmdtable)

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
    urlopener = urlmod.opener(repo.ui, authinfo)
    repo.ui.debug('Conduit Call: %s %s\n' % (url, params))
    params = params.copy()
    params['api.token'] = token
    request = util.urlreq.request(url, data=urlencodenested(params))
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
    '^Differential Revision:\s*(.*)D([1-9][0-9]*)$', re.M)

def getoldnodedrevmap(repo, nodelist):
    """find previous nodes that has been sent to Phabricator

    return {node: (oldnode or None, Differential Revision ID)}
    for node in nodelist with known previous sent versions, or associated
    Differential Revision IDs.

    Examines all precursors and their tags. Tags with format like "D1234" are
    considered a match and the node with that tag, and the number after "D"
    (ex. 1234) will be returned.

    If tags are not found, examine commit message. The "Differential Revision:"
    line could associate this changeset to a Differential Revision.
    """
    url, token = readurltoken(repo)
    unfi = repo.unfiltered()
    nodemap = unfi.changelog.nodemap

    result = {} # {node: (oldnode or None, drev)}
    toconfirm = {} # {node: (oldnode, {precnode}, drev)}
    for node in nodelist:
        ctx = unfi[node]
        # For tags like "D123", put them into "toconfirm" to verify later
        precnodes = list(obsolete.allprecursors(unfi.obsstore, [node]))
        for n in precnodes:
            if n in nodemap:
                for tag in unfi.nodetags(n):
                    m = _differentialrevisiontagre.match(tag)
                    if m:
                        toconfirm[node] = (n, set(precnodes), int(m.group(1)))
                        continue

        # Check commit message (make sure URL matches)
        m = _differentialrevisiondescre.search(ctx.description())
        if m:
            if m.group(1).rstrip('/') == url.rstrip('/'):
                result[node] = (None, int(m.group(2)))
            else:
                unfi.ui.warn(_('%s: Differential Revision URL ignored - host '
                               'does not match config\n') % ctx)

    # Double check if tags are genuine by collecting all old nodes from
    # Phabricator, and expect precursors overlap with it.
    if toconfirm:
        confirmed = {} # {drev: {oldnode}}
        drevs = [drev for n, precs, drev in toconfirm.values()]
        diffs = callconduit(unfi, 'differential.querydiffs',
                            {'revisionIDs': drevs})
        for diff in diffs.values():
            drev = int(diff[r'revisionID'])
            oldnode = bin(encoding.unitolocal(getdiffmeta(diff).get(r'node')))
            if node:
                confirmed.setdefault(drev, set()).add(oldnode)
        for newnode, (oldnode, precset, drev) in toconfirm.items():
            if bool(precset & confirmed.get(drev, set())):
                result[newnode] = (oldnode, drev)
            else:
                tagname = 'D%d' % drev
                tags.tag(repo, tagname, nullid, message=None, user=None,
                         date=None, local=True)
                unfi.ui.warn(_('D%s: local tag removed - does not match '
                               'Differential history\n') % drev)

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
                               actions=None):
    """create or update a Differential Revision

    If revid is None, create a new Differential Revision, otherwise update
    revid. If parentrevid is not None, set it as a dependency.

    If oldnode is not None, check if the patch content (without commit message
    and metadata) has changed before creating another diff.

    If actions is not None, they will be appended to the transaction.
    """
    repo = ctx.repo()
    if oldnode:
        diffopts = mdiff.diffopts(git=True, context=1)
        oldctx = repo.unfiltered()[oldnode]
        neednewdiff = (getdiff(ctx, diffopts) != getdiff(oldctx, diffopts))
    else:
        neednewdiff = True

    transactions = []
    if neednewdiff:
        diff = creatediff(ctx)
        writediffproperties(ctx, diff)
        transactions.append({'type': 'update', 'value': diff[r'phid']})

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

    return revision

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
          ('', 'reviewer', [], _('specify reviewers'))],
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
    """
    revs = list(revs) + opts.get('rev', [])
    revs = scmutil.revrange(repo, revs)

    if not revs:
        raise error.Abort(_('phabsend requires at least one changeset'))

    actions = []
    reviewers = opts.get('reviewer', [])
    if reviewers:
        phids = userphids(repo, reviewers)
        actions.append({'type': 'reviewers.add', 'value': phids})

    oldnodedrev = getoldnodedrevmap(repo, [repo[r].node() for r in revs])

    # Send patches one by one so we know their Differential Revision IDs and
    # can provide dependency relationship
    lastrevid = None
    for rev in revs:
        ui.debug('sending rev %d\n' % rev)
        ctx = repo[rev]

        # Get Differential Revision ID
        oldnode, revid = oldnodedrev.get(ctx.node(), (None, None))
        if oldnode != ctx.node():
            # Create or update Differential Revision
            revision = createdifferentialrevision(ctx, revid, lastrevid,
                                                  oldnode, actions)
            newrevid = int(revision[r'object'][r'id'])
            if revid:
                action = _('updated')
            else:
                action = _('created')

            # Create a local tag to note the association
            tagname = 'D%d' % newrevid
            tags.tag(repo, tagname, ctx.node(), message=None, user=None,
                     date=None, local=True)
        else:
            # Nothing changed. But still set "newrevid" so the next revision
            # could depend on this one.
            newrevid = revid
            action = _('skipped')

        ui.write(_('D%s: %s - %s: %s\n') % (newrevid, action, ctx,
                                            ctx.description().split('\n')[0]))
        lastrevid = newrevid

# Map from "hg:meta" keys to header understood by "hg import". The order is
# consistent with "hg export" output.
_metanamemap = util.sortdict([(r'user', 'User'), (r'date', 'Date'),
                              (r'node', 'Node ID'), (r'parent', 'Parent ')])

def querydrev(repo, params, stack=False):
    """return a list of "Differential Revision" dicts

    params is the input of "differential.query" API, and is expected to match
    just a single Differential Revision.

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

    If stack is True, return a list of "Differential Revision dict"s in an
    order that the latter ones depend on the former ones. Otherwise, return a
    list of a unique "Differential Revision dict".
    """
    prefetched = {} # {id or phid: drev}
    def fetch(params):
        """params -> single drev or None"""
        key = (params.get(r'ids') or params.get(r'phids') or [None])[0]
        if key in prefetched:
            return prefetched[key]
        # Otherwise, send the request. If we're fetching a stack, be smarter
        # and fetch more ids in one batch, even if it could be unnecessary.
        batchparams = params
        if stack and len(params.get(r'ids', [])) == 1:
            i = int(params[r'ids'][0])
            # developer config: phabricator.batchsize
            batchsize = repo.ui.configint('phabricator', 'batchsize', 12)
            batchparams = {'ids': range(max(1, i - batchsize), i + 1)}
        drevs = callconduit(repo, 'differential.query', batchparams)
        # Fill prefetched with the result
        for drev in drevs:
            prefetched[drev[r'phid']] = drev
            prefetched[int(drev[r'id'])] = drev
        if key not in prefetched:
            raise error.Abort(_('cannot get Differential Revision %r') % params)
        return prefetched[key]

    visited = set()
    result = []
    queue = [params]
    while queue:
        params = queue.pop()
        drev = fetch(params)
        if drev[r'id'] in visited:
            continue
        visited.add(drev[r'id'])
        result.append(drev)
        if stack:
            auxiliary = drev.get(r'auxiliary', {})
            depends = auxiliary.get(r'phabricator:depends-on', [])
            for phid in depends:
                queue.append({'phids': [phid]})
    result.reverse()
    return result

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

def readpatch(repo, params, write, stack=False):
    """generate plain-text patch readable by 'hg import'

    write is usually ui.write. params is passed to "differential.query". If
    stack is True, also write dependent patches.
    """
    # Differential Revisions
    drevs = querydrev(repo, params, stack)

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
         _('REVID [OPTIONS]'))
def phabread(ui, repo, revid, **opts):
    """print patches from Phabricator suitable for importing

    REVID could be a Differential Revision identity, like ``D123``, or just the
    number ``123``, or a full URL like ``https://phab.example.com/D123``.

    If --stack is given, follow dependencies information and read all patches.
    """
    try:
        revid = int(revid.split('/')[-1].replace('D', ''))
    except ValueError:
        raise error.Abort(_('invalid Revision ID: %s') % revid)
    readpatch(repo, {'ids': [revid]}, ui.write, opts.get('stack'))
