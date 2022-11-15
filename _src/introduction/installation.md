---
sidebar_position: 10
---

import {gitHubRepo, gitHubRepoName, latestReleasePage} from '@site/constants'

import {Command, SLCommand} from '@site/elements'

import {macArmAsset, macIntelAsset, windowsAsset} from '@site/src/releaseData';

import CodeBlock from '@theme/CodeBlock';

# Installation

## Prebuilt Binaries

### Linux

#### Ubuntu

<p>Head over to the <a href={latestReleasePage}>latest release</a> and download the file ending in <code>Ubuntu20.04.deb</code> or <code>Ubuntu22.04.deb</code>, as appropriate, for your platform. You can use <code>apt</code> to install it, as follows:</p>

```
sudo apt install -y ~/Downloads/path-to-the-Ubuntu.deb
```

### macOS

First, make sure that [Homebrew](https://brew.sh/) is installed on your system. Then follow the instructions depending on your architecture.

#### Apple Silicon (arm64)

Download using `curl`:

<CodeBlock>
curl -L -O {macArmAsset.url}
</CodeBlock>

Then install:

<CodeBlock>
brew install ./{macArmAsset.name}
</CodeBlock>

#### Intel (x86_64)

Download using `curl`:

<CodeBlock>
curl -L -O {macIntelAsset.url}
</CodeBlock>

Then install:

<CodeBlock>
brew install ./{macIntelAsset.name}
</CodeBlock>

Note that to clone larger repositories, you need to change the open files limit. We recommend doing it now so it doesn't bite you in the future:

<CodeBlock>
echo "ulimit -n 1048576 1048576" >> ~/.bash_profile{'\n'}
echo "ulimit -n 1048576 1048576" >> ~/.zshrc
</CodeBlock>

:::caution

Downloading the bottle using a web browser instead of `curl` will cause macOS to tag Sapling as "untrusted" and the security manager will prevent you from running it. You can remove this annotation as follows:

<CodeBlock>
xattr -r -d com.apple.quarantine ~/Downloads/{macArmAsset.name}
</CodeBlock>

:::

### Windows

After downloading the `sapling_windows` ZIP from the <a href={latestReleasePage}>latest release</a>, run the following in PowerShell as Administrator (substituting the name of the `.zip` file you downloaded, as appropriate):

<CodeBlock>
PS> Expand-Archive ~/Downloads/{windowsAsset.name} 'C:\Program Files'{'\n'}
</CodeBlock>

This will create `C:\Program Files\Sapling`, which you likely want to add to your `%PATH%` environment variable using:

<CodeBlock>
PS> setx PATH "$env:PATH;C:\Program Files\Sapling" -m
</CodeBlock>

Note the following tools must be installed to leverage Sapling's full feature set:

- [Git for Windows](https://git-scm.com/download/win) is required to use Sapling with Git repositories
- [Node.js](https://nodejs.org/en/download/) (v10 or later) is required to use <SLCommand name="web" />

Note that the name of the Sapling CLI `sl.exe` conflicts with the `sl` shell built-in in PowerShell (`sl` is an alias for `Set-Location`, which is equivalent to `cd`). If you want to use `sl` to run `sl.exe` in PowerShell, you must reassign the alias. Again, you must run the following as Administrator:

```
PS> Set-Alias -Name sl -Value 'C:\Program Files\Sapling\sl.exe' -Force -Option Constant,ReadOnly,AllScope
PS> sl --version
Sapling 20221108.091155.887dee39
```

## Building from Source

In order to build from source, you need at least the following tools available in your environment:

- Make
- `g++`
- [Rust](https://www.rust-lang.org/tools/install)
- [Node.js](https://nodejs.org)
- [Yarn](https://yarnpkg.com/getting-started/install)

For the full list, find the appropriate `Dockerfile` for your platform that defines the image that is used for Sapling builds in automation to see which tools it installs. For example, <a href={`${gitHubRepo}/blob/main/.github/workflows/sapling-cli-ubuntu-22.04.Dockerfile`}><code>.github/workflows/sapling-cli-ubuntu-22.04.Dockerfile</code></a> reveals all of the packages you need to install via `apt-get` in the host environment in order to build Sapling from source.

Once you have your environment set up, you can do a build as follows:

<pre>{`\
git clone ${gitHubRepo}
cd ${gitHubRepoName}/eden/scm
make oss
./sl --help
`}
</pre>

Once you have Sapling installed, follow the [Getting Started](./getting-started.md) instructions.
