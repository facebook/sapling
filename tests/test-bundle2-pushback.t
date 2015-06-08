  $ cat > bundle2.py << EOF
  > """A small extension to test bundle2 pushback parts.
  > Current bundle2 implementation doesn't provide a way to generate those
  > parts, so they must be created by extensions.
  > """
  > from mercurial import bundle2, pushkey, exchange, util
  > def _newhandlechangegroup(op, inpart):
  >     """This function wraps the changegroup part handler for getbundle.
  >     It issues an additional pushkey part to send a new
  >     bookmark back to the client"""
  >     result = bundle2.handlechangegroup(op, inpart)
  >     if 'pushback' in op.reply.capabilities:
  >         params = {'namespace': 'bookmarks',
  >                   'key': 'new-server-mark',
  >                   'old': '',
  >                   'new': 'tip'}
  >         encodedparams = [(k, pushkey.encode(v)) for (k,v) in params.items()]
  >         op.reply.newpart('pushkey', mandatoryparams=encodedparams)
  >     else:
  >         op.reply.newpart('output', data='pushback not enabled')
  >     return result
  > _newhandlechangegroup.params = bundle2.handlechangegroup.params
  > bundle2.parthandlermapping['changegroup'] = _newhandlechangegroup
  > EOF

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = dummyssh
  > username = nobody <no.reply@example.com>
  > 
  > [alias]
  > tglog = log -G -T "{desc} [{phase}:{node|short}]"
  > EOF

Set up server repository

  $ hg init server
  $ cd server
  $ echo c0 > f0
  $ hg commit -Am 0
  adding f0

Set up client repository

  $ cd ..
  $ hg clone ssh://user@dummy/server client -q
  $ cd client

Enable extension
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > bundle2=$TESTTMP/bundle2.py
  > [experimental]
  > bundle2-exp = True
  > EOF

Without config

  $ cd ../client
  $ echo c1 > f1
  $ hg commit -Am 1
  adding f1
  $ hg push
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: pushback not enabled
  $ hg bookmark
  no bookmarks set

  $ cd ../server
  $ hg tglog
  o  1 [public:2b9c7234e035]
  |
  @  0 [public:6cee5c8f3e5b]
  



With config

  $ cd ../client
  $ echo '[experimental]' >> .hg/hgrc
  $ echo 'bundle2.pushback = True' >> .hg/hgrc
  $ echo c2 > f2
  $ hg commit -Am 2
  adding f2
  $ hg push
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ hg bookmark
     new-server-mark           2:0a76dfb2e179

  $ cd ../server
  $ hg tglog
  o  2 [public:0a76dfb2e179]
  |
  o  1 [public:2b9c7234e035]
  |
  @  0 [public:6cee5c8f3e5b]
  



