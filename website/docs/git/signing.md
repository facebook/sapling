---
sidebar_position: 4
---

import {SLCommand} from '@site/elements'

# Signing Commits

Currently, signing is only supported with commits in Git repos. See [Git's documentation on "Signing Your Work" for more context](https://git-scm.com/book/en/v2/Git-Tools-Signing-Your-Work).

Note that Sapling has a single configuration for your identity:

```
$ sl config ui.username
Alyssa P. Hacker <alyssa@example.com>
```

whereas Git has these as separate items:

```
$ git config user.name
Alyssa P. Hacker
$ git config user.email
alyssa@example.com
```

You must ensure that:

- Your value of `ui.username` can be parsed as `NAME <EMAIL>`.
- When parsed, these values match what you specified for **Real name** and **Email address** when you created your GPG key.

In Git, you would configure your repo for automatic signing via:

```
git config --local user.signingkey B577AA76BAE505B1
git config --local commit.gpgsign true
```

Because Sapling does not read values from `git config`, you must add the analogous configuration to Sapling as follows:

```
sl config --local gpg.key B577AA76BAE505B1
```

Sapling's equivalent to Git's `commit.gpgsign` config is `gpg.enabled`, but it
defaults to `true`.

Note that `--local` is used to enable signing for the *current* repository. Use `--user` to default to signing for *all* repositories on your machine.

## Limitations

Support for signing commits is relatively new in Sapling, so we only support a subset of Git's functionality, for now. Specifically:

- There is no `-S` option for <SLCommand name="commit" /> or other commands, as signing is expected to be set for the repository. To disable signing for an individual action, leveraging the `--config` flag like so should work, but has not been heavily tested:

```
sl --config gpg.enabled=false <command> <args>
```

- While Git supports multiple signing schemes ([GPG, SSH, or X.509](https://docs.github.com/en/authentication/managing-commit-signature-verification/telling-git-about-your-signing-key)), Sapling supports only GPG at this time.

## Troubleshooting

The Git documentation on GPG is a bit light on detail when it comes to ensuring you have GPG configured correctly.

First, make sure that `gpg` is available on your `$PATH` and that `gpg --list-secret-keys --keyid-format LONG` lists the keys you expect. Note that you will have to run `gpg --gen-key` to create a key that matches your Sapling identity if you do not have one available already.

A basic test to ensure that `gpg` is setup correctly is to use it to sign a pice of test data:

```
echo "test" | gpg --clearsign
```

If you see `error: gpg failed to sign the data`, try this StackOverflow article:

https://stackoverflow.com/questions/39494631/gpg-failed-to-sign-the-data-fatal-failed-to-write-commit-object-git-2-10-0

If you see `gpg: signing failed: Inappropriate ioctl for device`, try:

```
export GPG_TTY=$(tty)
```
