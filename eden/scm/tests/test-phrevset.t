#modern-config-incompatible

#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > fbcodereview=
  > EOF
  $ enable amend
  $ sl init repo
  $ cd repo
  $ echo 1 > 1
  $ sl add 1
  $ HGPLAIN=1 sl commit -m "$(printf 'title\n\nDifferential Revision: http.ololo.com/D1234')"
  $ sl up -q 'desc(title)'
  $ sl up D1234
  phrevset.callsign is not set - doing a linear search
  This will be slow if the diff was not committed recently
  abort: phrevset.graphqlonly is set and Phabricator cannot resolve D1234
  [255]

  $ drawdag << 'EOS'
  > A  > EOS
  $ setconfig phrevset.mock-D1234=$A phrevset.callsign=CALLSIGN
  $ sl log -r D1234 -T '{desc}\n'
  A

# Callsign is invalid

  $ sl log -r D1234 --config phrevset.callsign=C -T '{desc}\n'
  abort: Diff callsign 'CALLSIGN' does not match repo callsigns '['C']'
  [255]

# Now we have two callsigns, and one of them is correct. Make sure it works

  $ sl log -r D1234 --config phrevset.callsign=C,CALLSIGN -T '{desc}\n'
  A

# Callsign set by .arcconfig works when phrevset.callsign is absent

  $ echo '{"repository.callsign":"CALLSIGN"}' > .arcconfig
  $ sl commit -m 'add arcconfig' -A .arcconfig
  $ sl log -r D1234 --config phrevset.callsign= -T '{desc}\n'
  A

# Phabricator provides an unknown commit hash.

  $ setconfig phrevset.mock-D1234=6008bb23d775556ff6c3528541ca5a2177b4bb92
  $ sl log -r D1234 -T '{desc}\n'
  abort: unknown revision 'D1234'!
  [255]

# 'pull -r Dxxx' will be rewritten to 'pull -r HASH'

  $ sl pull -r D1234 --config paths.default=test:fake_server
  pulling from test:fake_server
  rewriting pull rev 'D1234' into '6008bb23d775556ff6c3528541ca5a2177b4bb92'
  abort: unknown revision '6008bb23d775556ff6c3528541ca5a2177b4bb92'!
  [255]

# Ambiguous local successors warn and default to the newest revision.

  $ setconfig mutation.record=true mutation.enabled=true phrevset.mock-local-D1234=$A
  $ sl goto -q $A
  $ HGPLAIN=1 sl amend -q -d "1000 0" -m "$(printf 'older successor\n\nDifferential Revision: https://phabricator.intern.facebook.com/D1234')"
  $ sl goto -q --hidden --config commit.reject-modifying-obsolete=false $A
  $ HGPLAIN=1 sl amend -q -d "2000 0" -m "$(printf 'newer successor\n\nDifferential Revision: https://phabricator.intern.facebook.com/D1234')"

  $ sl log -r D1234 -T '{desc|firstline}\n'
  D1234 resolves ambiguously to multiple local commits:
    1: * 1970-01-01 00:33:20 +0000 newer successor (glob)
    2: * 1970-01-01 00:16:40 +0000 older successor (glob)
  which commit to select [1-2/(c)ancel]?  1
  warning: selected * for D1234 (glob)
  newer successor

# An interactive user may still cancel.

  $ sl log -r D1234 --config ui.interactive=true -T '{desc|firstline}\n' <<EOF
  > c
  > EOF
  D1234 resolves ambiguously to multiple local commits:
    1: * 1970-01-01 00:33:20 +0000 newer successor (glob)
    2: * 1970-01-01 00:16:40 +0000 older successor (glob)
  which commit to select [1-2/(c)ancel]?  c
  abort: ambiguous commit for D1234
  (set 'phrevset.prompt-ambiguous-successors=false' to restore automatic selection)
  [255]

# Plain mode preserves newest-revision selection for automation.

  $ HGPLAIN=1 sl log -r D1234 -T '{desc|firstline}\n'
  newer successor

  $ sl log -r D1234 --config ui.interactive=true -T '{desc|firstline}\n' <<EOF
  > 2
  > EOF
  D1234 resolves ambiguously to multiple local commits:
    1: * 1970-01-01 00:33:20 +0000 newer successor (glob)
    2: * 1970-01-01 00:16:40 +0000 older successor (glob)
  which commit to select [1-2/(c)ancel]?  2
  warning: selected * for D1234 (glob)
  older successor

# The config override preserves newest-revision selection.

  $ sl log -r D1234 --config phrevset.prompt-ambiguous-successors=false -T '{desc|firstline}\n'
  newer successor

# Agents should abort instead of guessing between ambiguous successors.

  $ CODING_AGENT_METADATA=id=test_agent sl log -r D1234 -T '{desc|firstline}\n'
  D1234 resolves ambiguously to multiple local commits:
    1: * 1970-01-01 00:33:20 +0000 newer successor (glob)
    2: * 1970-01-01 00:16:40 +0000 older successor (glob)
  abort: ambiguous commit for D1234
  (specify one of the commit hashes directly, or set 'phrevset.prompt-ambiguous-successors=false' to select the newest revision automatically)
  [255]

# The config override restores automatic selection for agents.

  $ CODING_AGENT_METADATA=id=test_agent sl log -r D1234 --config phrevset.prompt-ambiguous-successors=false -T '{desc|firstline}\n'
  newer successor
