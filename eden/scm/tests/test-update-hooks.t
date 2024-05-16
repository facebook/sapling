#debugruntest-incompatible
  $ configure modernclient

Make sure "goto" and "update" hooks support each other:
  $ newclientrepo
  $ hg go -q . --config 'hooks.pre-goto=echo worked'
  worked
  $ hg up -q . --config 'hooks.pre-goto=echo worked'
  worked
  $ hg go -q . --config 'hooks.pre-update=echo worked'
  worked
  $ hg up -q . --config 'hooks.pre-update=echo worked'
  worked
