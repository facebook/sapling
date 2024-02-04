## SCM daemon is a small program for polling Commit Cloud Updates.

#### Overview.

Commit Cloud Pub Sub system consists of two interngraph endpoints:

* https://interngraph.intern.facebook.com/commit_cloud/updates/poll
* https://interngraph.intern.facebook.com/commit_cloud/updates/publish

Our endpoints support both CATs and OAUTH tokens.

Publishing is done from the Source Control Service whenever `update_references` request has been accepted and the version of a Commit Cloud Workspace has been bumped.

Scm Daemon is responsible for subscribing (implemented as polling at a small specified interval giving us the ability to have a 'real-time' effect and syncronise Commit Cloud workspaces fast). Scm Daemon would trigger `hg cloud sync` command in the repos subscribed once a new version notification arrives.

#### How Scm Daemon knows list of Commit Cloud Workspaces to subscribe to?

List of workspaces/repos Scm Daemon needs to subscribe to is communicated directly by hg via a filesystem.

The path is usually `~/.commitcloud/joined/` but can be re-configured at `scm_daemon.dev.toml` (for local development) or `scm_daemon.toml` (`./fb/staticfiles/etc/mercurial/scm_daemon.toml`) using the following config:

```
[commitcloud]
connected_subscribers_path=/other/path
```

The exact same option must be set on the hg side in `~/.hgrc` if you would like to change the default path used.


#### How to build and run it locally?

from `~/fbsource/fbcode/eden/scm`

```
make local

RUST_LOG=debug ./build/scripts-3.8/scm_daemon --config `pwd`/fb/scm_daemon.dev.toml
```

using buck is also an option

```
RUST_LOG=debug buck run @mode/opt  //eden/scm/exec/scm_daemon:scm_daemon  -- --config `pwd`/fb/scm_daemon.dev.toml
```

To trigger a notification simply amend any commit in a repo or run hg pull.



#### How to interract with the endpoints directly?

It is absolutely possible to interract with the endpoints directly, that can be useful if you need to change them.


The commands are:

```
cd  ~/fbsource/fbcode/scm/commitcloud/client
```

```
export CAT_APP="184975892288525"
export CAT=$(clicat create --token_timeout_seconds 5800 --verifier_id interngraph --verifier_type SERVICE_IDENTITY --payload $(echo '{"app":184975892288525}' | base64))
```

Polling updates for the Commit Cloud Workspace `user/test/default` in the `fbsource` repo:

```
curl "https://interngraph.intern.facebook.com/commit_cloud/updates/poll?cat_app=$CAT_APP&crypto_auth_tokens=$CAT&repo_name=fbsource&workspace=user%2Ftest%2Fdefault"
```

Publishing a test update for the Commit Cloud Workspace `user/test/default` in the `fbsource` repo:

```
curl --data-binary '@notification_data.json' "https://interngraph.intern.facebook.com/commit_cloud/updates/publish?cat_app=$CAT_APP&crypto_auth_tokens=$CAT&repo_name=fbsource&workspace=user%2Ftest%2Fdefault"  -H"Content-Type:application/binary"
```

For calling into your sandbox, please replace "intern" with your OnDemand id, for example, interngraph.34940.od.facebook.com
