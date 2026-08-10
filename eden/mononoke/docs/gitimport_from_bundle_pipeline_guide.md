# Gitimport from Bundle Pipeline — User Guide

## Overview

This guide explains how to use the Skycastle-based **gitimport-from-bundle** pipeline to import a Git repository into Mononoke from a Git bundle stored in Manifold. The pipeline automates:

1. Validating that the bundle exists in Manifold
2. Creating the repo in Mononoke (if it doesn't exist)
3. Cloning the repo from Mononoke
4. Downloading the Git bundle from Manifold
5. Unbundling and running gitimport
6. Forcing a Git server cache refresh
7. Verifying that all refs match between the bundle and Mononoke

All interaction is through the terminal/CLI.

---

## Prerequisites

You need the following on your Fedora machine (connected to Corp VPN):

1. **fbsource checkout** — An up-to-date checkout of the fbsource repository.

   ```bash
   # If you don't have one, follow the standard devserver setup.
   # If you already have one, pull the latest:
   cd ~/fbsource
   sl pull && sl goto master
   ```

2. **Manifold CLI** — Used to upload bundles to Manifold.

   ```bash
   # Install if not already available
   sudo dnf install fb-manifold-cli
   ```

   Verify it works:

   ```bash
   manifold --help
   ```

3. **Skycastle CLI** — Comes with `arc`. Verify:

   ```bash
   arc skycastle --help
   ```

4. **VPN connection** — You must be connected to the Corp VPN for all commands to work.

---

## Step 1: Prepare the Git Bundle

If you already have a `.bundle` or `.pack` file, skip to Step 2.

To create a bundle from an existing Git repository:

```bash
cd /path/to/your/git/repo

# Create a bundle containing all refs
git bundle create /tmp/my_repo.bundle --all

# Verify the bundle is valid
git bundle verify /tmp/my_repo.bundle
```

---

## Step 2: Upload the Git Bundle to Manifold

Upload the bundle file to a Manifold bucket so the pipeline can fetch it.

The signature is `put [localpath] bucketpath` — **local file first**, and the bucket and key joined into a single argument.

```bash
manifold --apikey <YOUR_API_KEY> put /tmp/my_repo.bundle <BUCKET_NAME>/<KEY_PATH>
```

**Example:**

```bash
manifold --apikey upstream_repo_bundles-key put /tmp/my_repo.bundle upstream_repo_bundles/flat/my_project_repo
```

**Verify the upload succeeded:**

```bash
manifold --apikey upstream_repo_bundles-key ls upstream_repo_bundles/flat/my_project_repo
```

You should see the file listed with its size.

**Parameters explained:**
- `<YOUR_API_KEY>` — The Manifold API key for the bucket (e.g., `upstream_repo_bundles-key`). Ask your team if you don't know it.
- `<BUCKET_NAME>` — The Manifold bucket name (e.g., `upstream_repo_bundles`).
- `<KEY_PATH>` — The path/key within the bucket where the bundle will be stored (e.g., `flat/my_project_repo`).

---

## Step 3: Trigger the Skycastle Pipeline

Navigate to your fbsource checkout and run:

```bash
cd ~/fbsource

arc skycastle schedule \
  //tools/skycastle/workflows2/mononoke/mononoke_gitimport_from_bundle.sky:gitimport_from_bundle \
  --flag repo_name="<REPO_NAME>" \
  --flag manifold_key="<KEY_PATH>"
```

### Required Inputs

| Flag | Description | Example |
|------|-------------|---------|
| `repo_name` | The Mononoke repo name. Use `<namespace>/<repo>` format. | `aosp/platform_build` |
| `manifold_key` | The key/path of the bundle within the bucket. | `flat/platform_build` |

### Optional Inputs

| Flag | Default | Description |
|------|---------|-------------|
| `manifold_bucket` | `upstream_repo_bundles` | The Manifold bucket where the bundle is stored. |
| `manifold_apikey` | `upstream_repo_bundles-key` | API key for Manifold access. |
| `oncall_name` | `rl_source_control` | Oncall team name used during repo creation. |
| `include_refs` | `""` (all refs) | Comma-separated list of refs to import. Leave empty to import all. |
| `hipster_group` | `""` | Hipster group for ACL setup during repo creation. |
| `gitimport_concurrency` | `20` | Number of concurrent gitimport operations. |

### Minimal Example

```bash
arc skycastle schedule \
  //tools/skycastle/workflows2/mononoke/mononoke_gitimport_from_bundle.sky:gitimport_from_bundle \
  --flag repo_name="aosp/platform_build" \
  --flag manifold_key="flat/platform_build"
```

### Full Example (with all flags explicit)

```bash
arc skycastle schedule \
  //tools/skycastle/workflows2/mononoke/mononoke_gitimport_from_bundle.sky:gitimport_from_bundle \
  --flag repo_name="aosp/platform_build" \
  --flag manifold_bucket="upstream_repo_bundles" \
  --flag manifold_key="flat/platform_build" \
  --flag manifold_apikey="upstream_repo_bundles-key" \
  --flag oncall_name="rl_source_control"
```

### Successful Output

On success, you will see output like:

```text
Success!
    Duration: 10 secs
    Invocation ID: BnlKsfoe
    Workflow run ID: 3422735716815416755
    Workflow run URL: https://www.internalfb.com/sandcastle/workflow/3422735716815416755
    Execution Job(s): https://www.internalfb.com/intern/sandcastle/job/58546795361304427/
```

**Save the Workflow run URL** — you will need it to check the status.

---

## Step 4: Check Pipeline Status

### Option A: Via CLI

```bash
# Check the status of a workflow run by ID
arc skycastle workflow-run get-status <WORKFLOW_RUN_ID>
```

Example:

```bash
arc skycastle workflow-run get-status 3422735716815416755
```

### Option B: Via Browser

Open the **Workflow run URL** from the scheduling output in your browser:

```text
https://www.internalfb.com/sandcastle/workflow/<WORKFLOW_RUN_ID>
```

This shows all 7 pipeline steps with their status (pending, running, passed, failed). Click on any step to see its logs.

### Option C: Via Execution Job URL

Open the **Execution Job URL** from the scheduling output:

```text
https://www.internalfb.com/intern/sandcastle/job/<JOB_ID>/
```

This provides detailed logs for each action in the pipeline.

---

## Pipeline Steps Explained

The pipeline runs 7 sequential steps:

| Step | Name | What It Does |
|------|------|-------------|
| 1 | **Validate Bundle** | Confirms the bundle exists in Manifold before any other work starts, so a bad key fails the run in seconds rather than after a repo has been created. |
| 2 | **Check or Create Repo** | Checks if the repo exists in Mononoke. If not, creates it via SCS thrift and polls for up to 45 minutes for the creation to complete. Re-submits up to 3 times if the config mutation dies. |
| 3 | **Clone from Mononoke** | Clones the repo from Mononoke's Git server. If the repo is empty (newly created), initializes a bare repo instead. |
| 4 | **Fetch Bundle** | Downloads the Git bundle from Manifold to the worker machine. |
| 5 | **Unbundle and Gitimport** | Unpacks the bundle into the cloned repo and runs `gitimport` to import all objects and refs into Mononoke. Retries up to 45 times with 60-second intervals if gitimport fails (e.g., waiting for config propagation). |
| 6 | **Bust Git Cache** | Pushes and deletes a temporary branch to force the Mononoke Git server to refresh its cache. |
| 7 | **Verify Import** | Compares refs from the bundle with refs on the Mononoke Git server to confirm everything was imported correctly. |

---

## Troubleshooting

### Repo creation uses wrong oncall

By default, repo creation uses the `rl_source_control` oncall. If you need a different oncall, override it with `--flag oncall_name="<your_oncall>"`.

### Repo creation: the config mutation died

Creating a repo raises a Configerator mutation, which the pipeline polls for up to 45 minutes. Two things can kill that mutation, and the pipeline handles both automatically:

- **`Canary failed`** — the config change failed its canary.
- **`SIMULTANEOUS_MODIFICATION`** — someone else landed a change to the same configs while the mutation was canarying. The mutation writes `repos/repo_index.cinc` and the shared tier manifests, which every repo creation touches, so the collision window is the whole ~20-minute canary phase. A concurrent pipeline run or an unrelated Mononoke config land can both cause it.

In either case SCS releases the repo's reservation before reporting the failure, so the pipeline re-submits `create_repos` from scratch and gets a fresh mutation. It tries up to 3 times, waiting 5–7 minutes between attempts. **No action is needed unless all three fail**, in which case the run reports the last failure reason.

### Repo creation timed out

The mutation was still in flight after 45 minutes of polling, without ever reporting a failure. Check whether SCS is healthy, and read the `create_repos_poll` responses in the Sandcastle job log — the timeout message now includes the last response. Re-running is safe: an existing repo is detected and skipped.

### Repo creation aborted after unrecognised responses

`create_repos_poll` returned five consecutive responses the pipeline could not classify — neither a known status nor a known failure. This usually means SCS changed its error wording; it is not something you caused.

The pipeline deliberately does **not** re-create here, because it cannot prove the mutation is dead, and re-creating a live mutation would leave two of them competing for the same repo. The abort message names the `mutation_id` and links the mutation. Check its state before re-running; if it is dead, the message explains how to clear the leftover reservation. Transport failures reaching SCS are handled separately and do not trigger this.

### Gitimport fails after all retries

Gitimport retries up to 45 times with 60-second intervals (total ~45 minutes). Common causes:
- **Config not propagated**: New repos need their config to propagate. The retry mechanism handles this, but if 45 minutes isn't enough, check config propagation manually.
- **Large repo**: For very large repos, *lower* concurrency with `--flag gitimport_concurrency=5`. The default was reduced from 200 to 20 because high concurrency was itself a cause of failures on big repos.
- Check the detailed logs in Sandcastle for the specific error message.

### Verification failed: refs do not match

The final step compares bundle refs with Mononoke Git server refs. If they don't match:
- **Extra refs in Mononoke**: The repo may have had pre-existing refs. This is usually fine.
- **Missing refs in Mononoke**: Gitimport may have skipped some refs. Check the gitimport logs for errors.
- **Cache not fully refreshed**: The cache-busting step may not have fully propagated. Wait a few minutes and manually check with:

  ```bash
  git ls-remote https://git.internal.tfbnw.net/repos/git/ro/<REPO_NAME>.git
  ```

### Manifold upload/download failures

- Verify your API key is correct: `manifold --apikey <KEY> ls <BUCKET>/<PATH>`
- Ensure the bundle file is not corrupted: `git bundle verify /path/to/bundle`
- Check that the Manifold bucket exists and you have write access.

### Clone fails during Step 2

- For newly created repos, clone failure is expected and handled automatically (the pipeline initializes a bare repo instead).
- If clone fails for an existing repo, check network connectivity to `git.internal.tfbnw.net`.

### "arc skycastle" command not found

Ensure `arc` is installed and in your PATH:

```bash
# On Fedora, install arc tools
sudo dnf install fb-arcanist
```

### VPN connectivity issues

All commands require Corp VPN. Verify connectivity:

```bash
# Test access to internal services
curl -s -o /dev/null -w "%{http_code}" https://git.internal.tfbnw.net
```

If you get connection errors, reconnect your VPN and retry.

### Pipeline infra failures and retries

The pipeline is configured with 3 automatic infra-level retries. If a failure is due to infrastructure issues (machine problems, network blips), it will automatically retry the entire workflow. User failures (bad input, missing permissions) will not be retried.
