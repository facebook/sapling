---
sidebar_position: 4
---

import {SLCommand} from '@site/elements'

# Signing commits

Currently, signing is only supported with commits in Git repos. See [Git's documentation on "Signing Your Work" for more context](https://git-scm.com/book/en/v2/Git-Tools-Signing-Your-Work). Sapling supports GPG, SSH, and X.509 (S/MIME) signing backends.

## Identity configuration (GPG)

When using GPG signing, Sapling has a single configuration for your identity:

```sh
$ sl config ui.username
Alyssa P. Hacker <alyssa@example.com>
```

whereas Git has these as separate items:

```sh
$ git config user.name
Alyssa P. Hacker
$ git config user.email
alyssa@example.com
```

You must ensure that:

- Your value of `ui.username` can be parsed as `NAME <EMAIL>`.
- When parsed, these values match what you specified for **Real name** and **Email address** when you created your GPG key.

:::note
This identity matching requirement applies only to GPG signing. SSH signing does not require `ui.username` to match the signing key.
:::

## Configuration

### Recommended: `[signing]` config

The `[signing]` section is the recommended way to configure commit signing. It supports GPG, SSH, and X.509 backends.

**SSH example:**

```
[signing]
backend = ssh
key = ~/.ssh/id_ed25519
```

**GPG example:**

```
[signing]
backend = gpg
key = B577AA76BAE505B1
```

**X.509 (S/MIME) example using openssl (recommended):**

```
[signing]
backend = x509
key = ~/certs/signing.pem
```

The `key` should be a PEM file containing both your X.509 certificate and private key. If your certificate and key are in separate files, use `x509.certfile`:

```
[signing]
backend = x509
key = ~/certs/private-key.pem
x509.certfile = ~/certs/certificate.pem
```

Sapling auto-detects `openssl` (available by default on macOS and most Linux distributions) or falls back to `gpgsm`. You can also explicitly set the program — see [X.509 program configuration](#x509-program-configuration) below.

**X.509 with gpgsm:**

If you prefer to use `gpgsm` (GnuPG's S/MIME tool, same as Git's `gpg.format=x509`), set the program explicitly:

```
[signing]
backend = x509
key = 0xABCDEF1234567890
x509.program = gpgsm
```

With `gpgsm`, the `key` value can be a key ID, certificate fingerprint, or email address matching a certificate in your `gpgsm` keyring.

Use `sl config --local` to enable signing for the *current* repository, or `--user` to default to signing for *all* repositories on your machine:

```sh
sl config --local signing.backend ssh
sl config --local signing.key ~/.ssh/id_ed25519
```

#### X.509 program configuration {#x509-program-configuration}

Sapling auto-detects the best available X.509 signing tool, trying `openssl` first and falling back to `gpgsm`. You can override this:

```
[signing]
x509.program = gpgsm
```

Note that the key format differs between tools:
- **openssl**: `key` is a file path to a PEM file (certificate + private key)
- **gpgsm**: `key` is a keyring identifier (key ID, fingerprint, or email)

#### SSH key formats for `signing.key`

The `signing.key` config accepts several formats:

- **File path to a private key** (most common): `~/.ssh/id_ed25519`
- **Literal public key** with `key::` prefix: `key::ssh-ed25519 AAAA...` — the private key is looked up via ssh-agent
- **Bare `ssh-` prefix**: `ssh-ed25519 AAAA...` — deprecated, use the `key::` prefix instead

### Legacy: `[gpg]` config

The legacy `[gpg]` section is still supported. If you already have this configured, there is no need to migrate.

In Git, you would configure your repo for automatic signing via:

```sh
git config --local user.signingkey B577AA76BAE505B1
git config --local commit.gpgsign true
```

Because Sapling does not read values from `git config`, you must add the analogous configuration to Sapling as follows:

```sh
sl config --local gpg.key B577AA76BAE505B1
```

Sapling's equivalent to Git's `commit.gpgsign` config is `gpg.enabled`, but it defaults to `true`.

## Limitations

Support for signing commits is relatively new in Sapling, so we only support a subset of Git's functionality, for now. Specifically:

- There is no `-S` option for <SLCommand name="commit" /> or other commands, as signing is expected to be set for the repository. To disable signing for an individual action, leveraging the `--config` flag like so should work, but has not been heavily tested:

```sh
sl --config gpg.enabled=false <command> <args>
```

- While Git supports multiple signing schemes ([GPG, SSH, or X.509](https://docs.github.com/en/authentication/managing-commit-signature-verification/telling-git-about-your-signing-key)), Sapling supports all three: GPG, SSH, and X.509.

## Troubleshooting

### GPG

The Git documentation on GPG is a bit light on detail when it comes to ensuring you have GPG configured correctly.

First, make sure that `gpg` is available on your `$PATH` and that `gpg --list-secret-keys --keyid-format LONG` lists the keys you expect. Note that you will have to run `gpg --gen-key` to create a key that matches your Sapling identity if you do not have one available already.

A basic test to ensure that `gpg` is setup correctly is to use it to sign a piece of test data:

```sh
echo "test" | gpg --clearsign
```

If you see `error: gpg failed to sign the data`, try this StackOverflow article:

https://stackoverflow.com/questions/39494631/gpg-failed-to-sign-the-data-fatal-failed-to-write-commit-object-git-2-10-0

If you see `gpg: signing failed: Inappropriate ioctl for device`, try:

```sh
export GPG_TTY=$(tty)
```

### X.509 (S/MIME)

#### Using openssl (default)

Verify that `openssl` is available (it ships with macOS and most Linux distributions):

```sh
openssl version
```

A basic test to ensure signing works with your certificate:

```sh
echo "test" | openssl cms -sign -signer ~/certs/signing.pem -inkey ~/certs/signing.pem -binary -noattr -outform pem
```

This should output a PEM-encoded CMS signature. If it fails:

- Verify your PEM file contains both the certificate (`BEGIN CERTIFICATE`) and private key (`BEGIN PRIVATE KEY`)
- To create a combined PEM from separate files: `cat cert.pem key.pem > combined.pem`
- To convert a PKCS#12 bundle (`.p12` / `.pfx`) to PEM: `openssl pkcs12 -in bundle.p12 -out signing.pem -nodes`

#### Using gpgsm

If you prefer `gpgsm` (set `signing.x509.program = gpgsm`), make sure it is installed. On macOS: `brew install gnupg`. Check with:

```sh
gpgsm --version
```

List your available X.509 certificates:

```sh
gpgsm --list-keys
```

A basic test to ensure that X.509 signing works:

```sh
echo "test" | gpgsm --detach-sign --armor --local-user your-key-id
```

To import a PKCS#12 certificate bundle into the `gpgsm` keyring:

```sh
gpgsm --import your-certificate.p12
```

For more details on X.509 commit signing and verification, see [GitLab's X.509 signing documentation](https://docs.gitlab.com/user/project/repository/signed_commits/x509/).

### SSH

Make sure that `ssh-keygen` is available on your `$PATH` and that your version of OpenSSH supports the `-Y sign` flag (OpenSSH 8.2p1 or later). You can check with:

```sh
ssh -V
```

A basic test to ensure that SSH signing works is to sign a piece of test data:

```sh
echo "test" | ssh-keygen -Y sign -n git -f ~/.ssh/id_ed25519
```

This should output an SSH signature block. If it fails, verify that the key file exists and that ssh-agent is running if you use a passphrase-protected key.

To verify a signed commit, you can use `git verify-commit`. First, create an `allowed_signers` file containing the public keys you trust:

```
alyssa@example.com ssh-ed25519 AAAA...
```

Then configure Git to use it:

```sh
git config gpg.ssh.allowedSignersFile ~/.ssh/allowed_signers
git verify-commit <commit-hash>
```
