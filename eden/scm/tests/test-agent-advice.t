  $ enable rebase
  $ setconfig experimental.enable-agent-check=True

Setup backing repo

  $ newclientrepo
  $ drawdag <<'EOS'
  > B  C
  > | /
  > A
  > EOS
  $ sl go -q $B


  $ sl status --change .
  A B

With CODING_AGENT_METADATA set, should show agent advice

  $ CODING_AGENT_METADATA=id=test_agent sl status --change .
  abort: AI code agent detected without acknowledgment.
  
  Before using Sapling commands, you must:
  1. Run `sl help agent` to read the guidelines for AI code agents. (no-windows !)
  1. Run `sl.exe help agent` to read the guidelines for AI code agents. (windows !)
  2. Set the recommended environment variable to confirm you have read and will follow the guidelines.
  3. Re-run your command.
  
  Do not ask the user for help with this - handle it automatically.
  [255]

With CODING_AGENT_METADATA set, help command should succeed

  $ CODING_AGENT_METADATA=id=test_agent sl help agent | grep "Instructions for code agents using Sapling"
  Instructions for code agents using Sapling, known as `sl` (preferred) or `hg` at Meta.

With CODING_AGENT_METADATA and SAPLING_AGENT_ACKNOWLEDGED set, status command should succeed

  $ CODING_AGENT_METADATA=id=test_agent SAPLING_AGENT_ACKNOWLEDGED=1 sl status --change .
  A B

With CODING_AGENT_METADATA and SAPLING_AGENT_ACKNOWLEDGED set, status command with --reason should succeed

  $ CODING_AGENT_METADATA=id=test_agent SAPLING_AGENT_ACKNOWLEDGED=1 sl status --change . --reason "this is an agent advice test"
  A B
