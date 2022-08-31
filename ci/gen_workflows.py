#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import math
import sys
from pathlib import Path
from typing import Optional

import yaml

DOCKERFILE_DIR = ".github/workflows"

CARGO_FETCH_PATHS = [
    "eden/scm/exec/hgmain/Cargo.toml",
    "eden/scm/edenscmnative/conch_parser/Cargo.toml",
    "eden/scm/exec/scratch/Cargo.toml",
    "eden/scm/exec/scm_daemon/Cargo.toml",
]

UBUNTU_VERSIONS = ["20.04", "22.04"]

UBUNTU_VERSION_DEPS = {
    "20.04": [
        "python3.8",
        "python3.8-dev",
        "python3.8-distutils",
    ],
    "22.04": [
        "python3.10",
        "python3.10-dev",
        "python3.10-distutils",
    ],
}

UBUNTU_DEPS = [
    "nodejs",
    "pkg-config",
    "libssl-dev",
    "cython3",
    "make",
    "g++",
    "cargo",
    # This is needed for dpkg-name.
    "dpkg-dev",
]


def main() -> int:
    """Takes sys.argv[1] and uses it as the folder where all of the GitHub
    actions should be written.
    """
    out_dir = get_out_dir()
    if out_dir is None:
        eprint("must specify an output folder")
        sys.exit(1)

    workflows = WorkflowGenerator(out_dir)
    for ubuntu_version in UBUNTU_VERSIONS:
        workflows.gen_build_ubuntu_image(ubuntu_version=ubuntu_version)
        workflows.gen_ubuntu_ci(ubuntu_version=ubuntu_version)
        workflows.gen_ubuntu_release(ubuntu_version=ubuntu_version)
    return 0


def get_out_dir() -> Optional[Path]:
    try:
        out_dir = sys.argv[1]
    except IndexError:
        return None
    return Path(out_dir)


class WorkflowGenerator:
    def __init__(self, out_dir: Path):
        self.out_dir = out_dir

    def gen_build_ubuntu_image(self, *, ubuntu_version: str) -> None:
        image_name = f"ubuntu:{ubuntu_version}"
        dockerfile_name = f"sapling-cli-ubuntu-{ubuntu_version}.Dockerfile"
        cargo_prefetch_commands = "".join(
            [f"RUN cargo fetch --manifest-path {p}\n" for p in CARGO_FETCH_PATHS]
        )
        if cargo_prefetch_commands:
            cargo_prefetch_commands = (
                "\n# Run `cargo fetch` on the crates we plan to build.\n"
                + cargo_prefetch_commands
            )

        full_deps = UBUNTU_DEPS + UBUNTU_VERSION_DEPS[ubuntu_version]
        dockerfile = f"""\
FROM {image_name}

# https://serverfault.com/a/1016972 to ensure installing tzdata does not
# result in a prompt that hangs forever.
ARG DEBIAN_FRONTEND=noninteractive
ENV TZ=Etc/UTC

# Update and install some basic packages to register a PPA.
RUN apt-get -y update
RUN apt-get -y install curl git

# Use a PPA to ensure a specific version of Node (the default Node on
# Ubuntu 20.04 is v10, which is too old):
RUN curl -fsSL https://deb.nodesource.com/setup_16.x | bash -

# Now we can install the bulk of the packages:
RUN apt-get -y install {' '.join(full_deps)}

# Copy the full repo over because `cargo fetch` follows deps within the repo,
# so assume it needs everything.
COPY . /tmp/repo
WORKDIR /tmp/repo
{cargo_prefetch_commands}
# Create and populate a Yarn offline mirror by running `yarn install`
# in the addons/ folder that contains yarn.lock, package.json, and the
# package.json file for each entry in the Yarn workspace.
RUN npm install --global yarn
RUN yarn config set yarn-offline-mirror "$HOME/npm-packages-offline-cache"
RUN [ -f /tmp/repo/addons ] && yarn --cwd /tmp/repo/addons install || true

# Verify the yarn-offline-mirror was populated.
RUN find $(yarn config get yarn-offline-mirror)

# Clean up to reduce the size of the Docker image.
WORKDIR /root
RUN rm -rf /tmp/repo
"""
        out_file = self.out_dir / dockerfile_name
        with out_file.open("w") as f:
            f.write(dockerfile)

        container = self._get_ubuntu_container_name(ubuntu_version)
        gh_action_build_image = {
            "name": f"Docker Image - {image_name}",
            "on": "workflow_dispatch",
            "jobs": {
                "clone-and-build": {
                    "runs-on": "ubuntu-latest",
                    "steps": [
                        {"name": "Checkout Code", "uses": "actions/checkout@v3"},
                        {
                            "name": "Set up Docker Buildx",
                            "uses": "docker/setup-buildx-action@v2",
                        },
                        {
                            "name": "Login to GitHub Container Registry",
                            "uses": "docker/login-action@v2",
                            "with": {
                                "registry": "ghcr.io",
                                "username": "${{ github.repository_owner }}",
                                "password": "${{ secrets.GITHUB_TOKEN }}",
                            },
                        },
                        {
                            "name": "Build and Push Docker Image",
                            "uses": "docker/build-push-action@v3",
                            "with": {
                                "context": ".",
                                "file": f"{DOCKERFILE_DIR}/{dockerfile_name}",
                                "push": True,
                                "tags": container,
                            },
                        },
                    ],
                },
            },
        }
        self._write_file(
            f"sapling-cli-ubuntu-{ubuntu_version}-image.yml", gh_action_build_image
        )

    def gen_ubuntu_ci(self, *, ubuntu_version: str) -> str:
        gh_action = {
            "name": f"CI - Ubuntu {ubuntu_version}",
            "on": "workflow_dispatch",
            "jobs": {
                "build-deb": self.gen_build_ubuntu_cli_job(
                    ubuntu_version=ubuntu_version, deb_version_expr="v0"
                )
            },
        }
        self._write_file(f"sapling-cli-ubuntu-{ubuntu_version}-ci.yml", gh_action)

    def gen_ubuntu_release(self, *, ubuntu_version: str) -> str:
        """Logic for releases is modeled after wezterm's GitHub Actions:
        https://github.com/wez/wezterm/tree/6962d6805abf/.github/workflows

        Note that the build job is run in a Docker image, but the upload is
        done on the host, as empirically, that is more reliable.
        """

        BUILD = "build"
        artifact_key = f"ubuntu-{ubuntu_version}"
        build_job = self.gen_build_ubuntu_cli_job(
            ubuntu_version=ubuntu_version, deb_version_expr="${{ github.ref }}"
        )
        build_job["steps"].append(
            {
                "name": "Upload Artifact",
                "uses": "actions/upload-artifact@v3",
                "with": {"name": artifact_key, "path": "./eden/scm/sapling_*.deb"},
            }
        )

        publish_job = {
            "runs-on": "ubuntu-latest",
            "needs": BUILD,
            "steps": [
                {"name": "Checkout Code", "uses": "actions/checkout@v3"},
                grant_repo_access(),
                {
                    "name": "Download Artifact",
                    "uses": "actions/download-artifact@v3",
                    "with": {"name": artifact_key},
                },
                {
                    "name": "Create pre-release",
                    "env": {"GITHUB_TOKEN": "${{ secrets.GITHUB_TOKEN }}"},
                    "shell": "bash",
                    "run": "bash ci/retry.sh bash ci/create-release.sh $(ci/tag-name.sh)",
                },
                {
                    "name": "Upload Release",
                    "env": {"GITHUB_TOKEN": "${{ secrets.GITHUB_TOKEN }}"},
                    "shell": "bash",
                    "run": "bash ci/retry.sh gh release upload --clobber $(ci/tag-name.sh) sapling_*.deb",
                },
            ],
        }
        gh_action = {
            "name": f"Release - Ubuntu {ubuntu_version}",
            "on": {"push": {"tags": ["v*", "test-release-*"]}},
            "jobs": {
                BUILD: build_job,
                "publish": publish_job,
            },
        }
        self._write_file(f"sapling-cli-ubuntu-{ubuntu_version}-release.yml", gh_action)

    def gen_build_ubuntu_cli_job(self, *, ubuntu_version: str, deb_version_expr: str):
        container = self._get_ubuntu_container_name(ubuntu_version)
        # https://www.debian.org/doc/debian-policy/ch-controlfields.html#version
        # documents the constraints on this segment of the version number.
        DEB_UPSTREAM_VERISION = "DEB_UPSTREAM_VERISION"
        return {
            "runs-on": "ubuntu-latest",
            "container": {"image": container},
            "steps": [
                {"name": "Checkout Code", "uses": "actions/checkout@v3"},
                grant_repo_access(),
                create_set_env_step(
                    DEB_UPSTREAM_VERISION, "$(ci/tag-name.sh | tr \\- .)"
                ),
                {
                    "name": "Create .deb",
                    "working-directory": "./eden/scm",
                    "run": f"${{{{ format('VERSION=0.0-{{0}} make deb-ubuntu-{ubuntu_version}', env.{DEB_UPSTREAM_VERISION}) }}}}",
                },
                {
                    "name": "Rename .deb",
                    "working-directory": "./eden/scm",
                    "run": f"${{{{ format('mv sapling_0.0-{{0}}_amd64.deb sapling_0.0-{{0}}_amd64.Ubuntu{ubuntu_version}.deb', env.{DEB_UPSTREAM_VERISION}, env.{DEB_UPSTREAM_VERISION}) }}}}",
                },
            ],
        }

    def _get_ubuntu_container_name(self, version: str) -> str:
        """Name of container to use when doing builds on Ubuntu: will be built
        by "Docker Image" GitHub Action.
        """
        name = f"build_ubuntu_{version.replace('.', '_')}"
        return gen_container_name(name=name)

    def _write_file(self, filename: str, gh_action):
        path = self.out_dir / filename
        with path.open("w") as f:
            yaml.dump(gh_action, f, width=math.inf, sort_keys=False)


def gen_container_name(*, name: str, tag: Optional[str] = "latest") -> str:
    return f"${{{{ format('ghcr.io/{{0}}/{name}:{tag}', github.repository) }}}}"


def create_set_env_step(env_var: str, env_expr):
    """See https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions#setting-an-environment-variable"""
    return {
        "name": f"set-env {env_var}",
        "run": f'echo "{env_var}={env_expr}" >> $GITHUB_ENV',
    }


def grant_repo_access():
    return {
        "name": "Grant Access",
        "run": 'git config --global --add safe.directory "$PWD"',
    }


def eprint(*args, **kwargs):
    print(*args, file=sys.stderr, **kwargs)


if __name__ == "__main__":
    main()
