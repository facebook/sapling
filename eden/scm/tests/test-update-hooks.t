  $ configure modernclient

Make sure "goto" and "update" hooks support each other:
  $ newclientrepo
FIXME: hook should not be invoked twice
  $ hg go -q . --config 'hooks.pre-goto=echo worked'
  worked
  worked
  $ hg up -q . --config 'hooks.pre-goto=echo worked'
  worked
  worked
  $ hg go -q . --config 'hooks.pre-update=echo worked'
  worked
  $ hg up -q . --config 'hooks.pre-update=echo worked'
  worked
