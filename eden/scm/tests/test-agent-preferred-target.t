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

Agents are nudged toward the preferred target without blocking the update.

  $ CODING_AGENT_METADATA=id=test_agent sl goto -q master
  hint[preferred-target]: continuing with 'master'. Do not ask the user which target to use. 'all_orchestrators_fast' follows 'master' closely, so it is normally a safe substitute when exact 'master' is unnecessary. If you know this workflow does not require 'master', use 'sl goto all_orchestrators_fast' for faster builds. Otherwise stay on 'master' and continue.
  $ sl log -r . -T '{desc}\n'
  tip
  $ sl goto -q "$BASE"
  $ CODING_AGENT_METADATA=id=test_agent sl goto -q remote/master
  hint[preferred-target]: continuing with 'remote/master'. Do not ask the user which target to use. 'all_orchestrators_fast' follows 'master' closely, so it is normally a safe substitute when exact 'master' is unnecessary. If you know this workflow does not require 'master', use 'sl goto all_orchestrators_fast' for faster builds. Otherwise stay on 'master' and continue.
  $ sl log -r . -T '{desc}\n'
  tip
  $ sl goto -q "$BASE"
  $ CODING_AGENT_METADATA=id=test_agent sl goto -q all_orchestrators_fast
  $ sl log -r . -T '{desc}\n'
  tip

Unsetting the preferred target allows the update.

  $ sl goto -q "$BASE"
  $ CODING_AGENT_METADATA=id=test_agent sl --config experimental.preferred-target= goto -q master
  $ sl log -r . -T '{desc}\n'
  tip

Agents are also nudged before rebase fast-forwards.

  $ sl goto -q "$BASE"
  $ CODING_AGENT_METADATA=id=test_agent sl rebase -q -d master
  hint[preferred-target]: continuing with 'master'. Do not ask the user which target to use. 'all_orchestrators_fast' follows 'master' closely, so it is normally a safe substitute when exact 'master' is unnecessary. If you know this workflow does not require 'master', use 'sl rebase -d all_orchestrators_fast' for faster builds. Otherwise stay on 'master' and continue.
  $ sl log -r . -T '{desc}\n'
  tip
  $ sl goto -q "$BASE"
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
  hint[preferred-target]: continuing with 'release/main'. Do not ask the user which target to use. 'all_orchestrators_fast' follows 'release/main' closely, so it is normally a safe substitute when exact 'release/main' is unnecessary. If you know this workflow does not require 'release/main', use 'sl goto all_orchestrators_fast' for faster builds. Otherwise stay on 'release/main' and continue.
  $ sl goto -q "$BASE"
  $ CODING_AGENT_METADATA=id=test_agent sl --config remotenames.selectivepulldefault=release/main goto -q remote/release/main
  hint[preferred-target]: continuing with 'remote/release/main'. Do not ask the user which target to use. 'all_orchestrators_fast' follows 'release/main' closely, so it is normally a safe substitute when exact 'release/main' is unnecessary. If you know this workflow does not require 'release/main', use 'sl goto all_orchestrators_fast' for faster builds. Otherwise stay on 'release/main' and continue.
