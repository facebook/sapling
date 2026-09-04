#testcases rustcheckout pythoncheckout

#require no-eden

#if rustcheckout
  $ setconfig checkout.use-rust=true
#endif

#if pythoncheckout
  $ setconfig checkout.use-rust=false
#endif

  $ enable rebase
  $ setconfig remotenames.selectivepulldefault=master
  $ setconfig remotenames.hoist=remote
  $ newclientrepo

Create main and preferred targets that point to the same commit. The check must
use the target name rather than the resolved commit.

  $ echo base > file
  $ sl commit -Aqm base --date "1000000000 0"
  $ BASE=$(sl log -r . -T '{node}')
  $ echo tip >> file
  $ sl commit -Aqm tip --date "1234567890 0"
  $ sl push -q -r . --to master --create
  $ sl push -q -r . --to release/main --create
  $ sl bookmark master
  $ sl bookmark release/main
  $ sl bookmark release/master
  $ sl bookmark all_orchestrators_fast
  $ sl goto -q "$BASE"

Agent behavior is unchanged when the preferred target is unset.

  $ CODING_AGENT_METADATA=id=test_agent sl goto -q master
  $ sl log -r . -T '{desc}\n'
  tip
  $ sl goto -q "$BASE"

Configure the preferred target for the remaining checks.

  $ setconfig experimental.preferred-target=all_orchestrators_fast

Human callers can still update to the main bookmark.

  $ sl goto -q master
  $ sl goto -q "$BASE"

Plain-mode automation is not rejected, even when invoked by an agent.

  $ HGPLAIN=1 CODING_AGENT_METADATA=id=test_agent sl goto -q master
  $ sl goto -q "$BASE"

A slash in an ordinary bookmark does not make it a remote-qualified main.

  $ CODING_AGENT_METADATA=id=test_agent sl goto -q release/master
  $ sl log -r . -T '{desc}\n'
  tip
  $ CODING_AGENT_METADATA=id=test_agent sl goto -q "$BASE"

Date-only updates have no explicit target to check.

  $ CODING_AGENT_METADATA=id=test_agent sl goto -q --date 2009-02-13
  $ sl log -r . -T '{desc}\n'
  tip
  $ sl goto -q "$BASE"

Agents are directed to the preferred target for update.

  $ CODING_AGENT_METADATA=id=test_agent sl goto -q master
  abort: use 'all_orchestrators_fast' for faster builds, or retry with '--config experimental.preferred-target=' to allow 'master'
  ('all_orchestrators_fast' follows 'master' closely, so is normally a safe substitute)
  [255]
  $ CODING_AGENT_METADATA=id=test_agent sl goto -q remote/master
  abort: use 'all_orchestrators_fast' for faster builds, or retry with '--config experimental.preferred-target=' to allow 'remote/master'
  ('all_orchestrators_fast' follows 'master' closely, so is normally a safe substitute)
  [255]
  $ CODING_AGENT_METADATA=id=test_agent sl goto -q all_orchestrators_fast
  $ sl log -r . -T '{desc}\n'
  tip

Unsetting the preferred target allows the update.

  $ sl goto -q "$BASE"
  $ CODING_AGENT_METADATA=id=test_agent sl --config experimental.preferred-target= goto -q master
  $ sl log -r . -T '{desc}\n'
  tip

Agents are also directed to the preferred target before rebase fast-forwards.

  $ sl goto -q "$BASE"
  $ CODING_AGENT_METADATA=id=test_agent sl rebase -q -d master
  abort: use 'all_orchestrators_fast' for faster builds, or retry with '--config experimental.preferred-target=' to allow 'master'
  ('all_orchestrators_fast' follows 'master' closely, so is normally a safe substitute)
  [255]
  $ sl log -r . -T '{desc}\n'
  base
  $ CODING_AGENT_METADATA=id=test_agent sl rebase -q -d all_orchestrators_fast
  $ sl log -r . -T '{desc}\n'
  tip

Unsetting the preferred target allows the rebase.

  $ sl goto -q "$BASE"
  $ CODING_AGENT_METADATA=id=test_agent sl --config experimental.preferred-target= rebase -q -d master
  $ sl log -r . -T '{desc}\n'
  tip

Multi-segment main bookmark names are preserved when removing a remote prefix.

  $ sl goto -q "$BASE"
  $ CODING_AGENT_METADATA=id=test_agent sl --config remotenames.selectivepulldefault=release/main goto -q release/main
  abort: use 'all_orchestrators_fast' for faster builds, or retry with '--config experimental.preferred-target=' to allow 'release/main'
  ('all_orchestrators_fast' follows 'release/main' closely, so is normally a safe substitute)
  [255]
  $ CODING_AGENT_METADATA=id=test_agent sl --config remotenames.selectivepulldefault=release/main goto -q remote/release/main
  abort: use 'all_orchestrators_fast' for faster builds, or retry with '--config experimental.preferred-target=' to allow 'remote/release/main'
  ('all_orchestrators_fast' follows 'release/main' closely, so is normally a safe substitute)
  [255]
