  $ configure modernclient

Make sure "goto" and "update" hooks support each other:
  $ newclientrepo
  $ sl go -q . --config 'hooks.pre-goto=echo worked'
  worked
  $ sl up -q . --config 'hooks.pre-goto=echo worked'
  worked
  $ sl go -q . --config 'hooks.pre-update=echo worked'
  worked
  $ sl up -q . --config 'hooks.pre-update=echo worked'
  worked
