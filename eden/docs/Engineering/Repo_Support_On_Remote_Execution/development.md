# Hacking on SCM Repo Manager


## Local Development

The most effective way to verify changes in the SCM Repo Manager is by launching a local Remote Execution worker, and then send actions to it using the frecli.

The local Remote Execution worker will use the SCM Repo Manager built from the codebase. The script calls `buck build` under the hood.

*Notes: for the time being it is recomended to disable the Mononoke dogfooding tier and use prod.*

*Everything will be running as `root` including the SCM Repo Manager and the EdenFS daemon*

```
 liuba ⛅️  ~/fbsource/fbcode
 [🍊] →  ./remote_execution/scripts/start_local_worker_with_scm.sh
```

When sending an action, please ensure that you provide both the platform name and a revision (which is mandatory).
The engine is the key argument here, otherwise the action will be executed on the production tier.

```
 liuba ⛅️  ~/fbsource
 [🍇] → time frecli --engine $HOSTNAME:5000 --engine-rpc grpc --platform scm-repo-support -r "$(sl whereami)" exec command -- ls /fbsource/fbcode/eden
```

EdenFS daemon's logs will be located in the worker's home directory (a temporary directory in dev). 

Please, use this command to locate the logs:
```
 liuba ⛅️  ~
 [🍍] → ps ax | grep edenfs | grep /data/repos/workers/
```

It is also possible to run with a locally built EdenFs, Sapling or both.

Please, use the following commands:

```
 liuba ⛅️  ~/fbsource/fbcode
 [🍓] → buck build @//mode/opt //eden/scm:hg --out /tmp/hg

 liuba ⛅️  ~/fbsource/fbcode
 [🥭] → buck build @//mode/opt //eden/fs/service:edenfs --out /tmp/edenfs
```

Now we can start a local Remote Execution worker that will spin up SCM Repo Manager that will use the DEV executables.

```
 liuba ⛅️  ~/fbsource/fbcode
 [🍑] → export EDENFS_DEV_EXECUTABLE=/tmp/edenfs

 liuba ⛅️  ~/fbsource/fbcode
 [🍋] → export SAPLING_DEV_EXECUTABLE=/tmp/hg

 liuba ⛅️  ~/fbsource/fbcode
 [🍊] →  ./remote_execution/scripts/start_local_worker_with_scm.sh
```


## Cogwheel Tests




## Actions Replay
