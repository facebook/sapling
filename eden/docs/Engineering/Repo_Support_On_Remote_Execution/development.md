# Hacking on SCM Repo Manager


## Local Development

The most effective way to verify changes in the SCM Repo Manager is by launching a local Remote Execution worker, and then send actions to it using the frecli.

The local Remote Execution worker will use the SCM Repo Manager built from the codebase. The script calls `buck build` under the hood.

*Notes: for the time being it is recommended to disable the Mononoke dogfooding tier and use prod.*

*Everything will be running as `root` including the SCM Repo Manager and the EdenFS daemon*

```
 liuba ⛅️  ~/fbsource/fbcode
 [🍊] →  ./remote_execution/scripts/start_local_worker_with_scm.sh
```

When sending an action, please ensure that you provide both the platform name and a revision (which is mandatory).
The engine is the key argument here, otherwise the action will be executed on the production tier.

```
 liuba ⛅️  ~/fbsource
 [🍇] → time frecli --engine local --platform scm-repo-support -r "$(sl whereami)" exec command -- ls /fbsource/fbcode/eden
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

An E2E cogwheel test is defined at `remote_execution/cogwheel/platforms/scm-repo-support/scm_test.py`.

This Python class extends the basic Remote Execution action tests which verify that the Remote Execution worker can execute standard actions (e.g. read configs, enforce command timeouts, download basic files from CAS etc.)
and adds new test cases which are [specific for the SCM platform](https://fburl.com/code/lenlzyv6) such as executing Sapling command on the repository.

This test runs as part of the conveyor pipeline to validate pushes and uses [a separate capacity entitlement](https://fburl.com/capacity_portal/ytnyyqvo).

All fbpkgs are built as part of the test (SCM resource manager, SCM image, RE agent, RE CASd), so be careful when rolling out changes with dependencies between services.

## Actions Replay

Remote Execution provides a tool called `re_replay` for executing previous actions onto a set of workers.

The tool has 2 modes:
- `record`: read actions from scuba and write them to stdout/a target file
- `replay`: send actions to running workers

The tool will **only select previously successful actions**.

The following command illustates a basic example of how to use the tool locally.

The first part will read 100 actions from scuba and write them to a file:
```
[ioanbudea@devvm33012.lla0 ~]$ re_replay record -o out.file -l 100 filter --platform scm-repo-support
```
Now, we are ready to replay those actions onto a testing instance:
```
[ioanbudea@devvm33012.lla0 ~]$ re_replay replay -i out.file --override-capabilities platform=scm-repo-support,testing=my_new_worker
```
For more details about the available filters, please consult the tool CLI and help instructions.
This can also be automated into [replay tests for push safety](https://www.internalfb.com/wiki/Remote_Execution/engineering/compute/push_safety_with_replay/).
