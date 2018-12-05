# Eden hooks

When Eden is deployed, the output of `generate-hooks-dir` should be installed
in `/etc/eden/hooks`.

By default, Eden will look in `/etc/eden/hooks` for hooks. This can be
overridden by specifying `hooks` in the `[repository]` section of an
`~/.edenrc`.

Note that hooks may require additional configuration. Hook authors should
encourage users to specify such configuration in `~/.edenrc`. This can be
read from the hook via `eden config`.

The following files will be recognized in the `hooks` directory for the
appropriate event:

## post-clone
This will be run after `eden clone`. If the `<repo_type>` is `.hg`,
the script is responsible for creating the `.hg` directory in the root of the
Eden mount. It will receive the following arguments:

```
/etc/eden/hooks/post-clone <repo_type> <eden_checkout> <repo>
```

* `<repo_type>` is `hg` or `git`
* `<eden_checkout>` is the path to the mounted Eden checkout.
* `<repo>` is the path to the original Mercurial repository.
* `<hash>` is the hex id of the initial commit
