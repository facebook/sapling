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
    # This is needed for dpkg-name.
    "dpkg-dev",
]

MACOS_RELEASES = {
    "x86": {
        "target": "x86_64-apple-darwin",
        "python_bottle_hash": "919aeadea2828aad7ccd95538e4db27943f8c1fc3c185e5d19d4afd89b1a79ad",
        "openssl_bottle_hash": "d915175bedb146e38d7a2c95e86888a60a5058a5cd21f835813d43d1372a29d9",
    },
    "arm64": {
        "target": "aarch64-apple-darwin",
        "python_bottle_hash": "4e968bd8f28aee189bd829c2b9feabeb8c5edd95b949cf327ea2f1f62ac76e66",
        "openssl_bottle_hash": "c11b17c8b78efa46dac2d213cd7a7b3fff75f6f5e6d2ef2248345cd4a900b1c6",
    },
}

SAPLING_VERSION = "SAPLING_VERSION"


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

    workflows.gen_windows_release()

    for arch, opts in MACOS_RELEASES.items():
        workflows.gen_homebrew_macos_release(arch, **opts)

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

# Unfortunately, we cannot `apt install cargo` because at the time of this
# writing, it installs a version of cargo that is too old (1.59). Specifically,
# cargo <1.60 has a known issue with weak dependency features:
#
# https://github.com/rust-lang/cargo/issues/10623
#
# which is new Cargo syntax that was introduced in Rust 1.60:
#
# https://blog.rust-lang.org/2022/04/07/Rust-1.60.0.html
#
# and indeed one of our dependencies makes use of this feature:
# https://github.com/rust-phf/rust-phf/blob/250c6b456fe28c0c8213518d6bddfd972922fd53/phf/Cargo.toml#L22
#
# Realistically, the Rust ecosystem moves forward quickly, so installing via
# rustup is the most sustainable option.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
ENV PATH="/root/.cargo/bin:${{PATH}}"

# Copy the full repo over because `cargo fetch` follows deps within the repo,
# so assume it needs everything.
COPY . /tmp/repo
WORKDIR /tmp/repo

# Create and populate a Yarn offline mirror by running `yarn install`
# in the addons/ folder that contains yarn.lock, package.json, and the
# package.json file for each entry in the Yarn workspace.
RUN npm install --global yarn
RUN yarn config set yarn-offline-mirror "$HOME/npm-packages-offline-cache"
# If the addons/ folder is moved or no longer contains a package.json,
# this command will fail and should be updated to reflect the new location.
RUN yarn --cwd addons install --prefer-offline

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
            "on": {
                "workflow_dispatch": None,
                "schedule": [
                    # Every monday at 1am.
                    {"cron": "0 1 * * mon"},
                ],
            },
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
            upload_artifact(artifact_key, "./eden/scm/sapling_*.deb"),
        )

        publish_job = {
            "runs-on": "ubuntu-latest",
            "needs": BUILD,
            "steps": publish_release_steps(artifact_key, "sapling_*.deb"),
        }
        gh_action = {
            "name": f"Release - Ubuntu {ubuntu_version}",
            "on": release_trigger_on(),
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
        DEB_UPSTREAM_VERSION = "DEB_UPSTREAM_VERSION"
        return {
            "runs-on": "ubuntu-latest",
            "container": {"image": container},
            "steps": [
                {"name": "Checkout Code", "uses": "actions/checkout@v3"},
                grant_repo_access(),
                # This step feels as though it should be unnecessary because we
                # specified `--default-toolchain stable` when we ran rustup
                # originally in the Dockerfile. Nevertheless, without this step,
                # we see the following error message when trying to do the
                # build under GitHub Actions:
                #
                # error: rustup could not choose a version of cargo to run, because one wasn't specified explicitly, and no default is configured.
                # help: run 'rustup default stable' to download the latest stable release of Rust and set it as your default toolchain.
                #
                # It would be nice to debug this at some point, but it isn't pressing.
                {
                    "name": "rustup",
                    "run": "rustup default stable",
                },
                create_set_env_step(DEB_UPSTREAM_VERSION, "$(ci/tag-name.sh)"),
                create_set_env_step(SAPLING_VERSION, "$(ci/tag-name.sh)"),
                {
                    "name": "Create .deb",
                    "working-directory": "./eden/scm",
                    "run": f"${{{{ format('VERSION={{0}} make deb', env.{DEB_UPSTREAM_VERSION}) }}}}",
                },
                {
                    "name": "Rename .deb",
                    "working-directory": "./eden/scm",
                    "run": f"${{{{ format('mv sapling_{{0}}_amd64.deb sapling_{{0}}_amd64.Ubuntu{ubuntu_version}.deb', env.{DEB_UPSTREAM_VERSION}, env.{DEB_UPSTREAM_VERSION}) }}}}",
                },
            ],
        }

    def gen_windows_release(self) -> str:
        BUILD = "build"
        artifact_key = "windows-amd64"
        artifact_glob = "sapling_windows_*.zip"

        build_job = {
            "runs-on": "windows-latest",
            "steps": [
                {"name": "Checkout Code", "uses": "actions/checkout@v3"},
                grant_repo_access(),
                {"name": "rustup", "run": "rustup default stable"},
                # The "x64-windows-static-md" triple is what the Rust openssl
                # crate expects on Windows when it goes looking for the vcpkg
                # install.
                #
                # This step can probably be sped up with caching by using the
                # "run-vcpkg" action paired with a checked-in vcpkg manifest
                # file.
                {
                    "name": "openssl",
                    "run": "vcpkg install openssl:x64-windows-static-md",
                },
                # This makes vcpkg packages available globally.
                {"name": "integrate vcpkg", "run": "vcpkg integrate install"},
                create_set_env_step(SAPLING_VERSION, "$(ci/tag-name.sh)"),
                {
                    "name": "build and zip",
                    "run": "python3 ./eden/scm/packaging/windows/build_windows_zip.py",
                },
                {
                    "name": "rename .zip",
                    "working-directory": "./eden/scm/artifacts",
                    "run": "${{ format('Rename-Item sapling_windows_amd64.zip -NewName sapling_windows_{0}_amd64.zip', env.SAPLING_VERSION) }}",
                },
                upload_artifact(artifact_key, f"./eden/scm/artifacts/{artifact_glob}"),
            ],
        }

        publish_job = {
            "runs-on": "ubuntu-latest",
            "needs": BUILD,
            "steps": publish_release_steps(artifact_key, artifact_glob),
        }

        gh_action = {
            "name": "Release - Windows amd64",
            "on": release_trigger_on(),
            "jobs": {
                BUILD: build_job,
                "publish": publish_job,
            },
        }
        self._write_file("sapling-cli-windows-amd64-release.yml", gh_action)

    def gen_homebrew_macos_release(
        self, arch, target, python_bottle_hash, openssl_bottle_hash
    ) -> str:
        BUILD = "build"
        artifact_key = f"macos-homebrew-{arch}-bottle"
        mac_release = "monterey"
        extension = f"{mac_release}.bottle.tar.gz"
        final_ext = f"{'' if arch == 'x86' else 'arm64_'}{extension}"
        artifact_glob = "sapling*" + extension

        build_job = {
            "runs-on": "macos-12",
            "steps": [
                {"name": "Checkout Code", "uses": "actions/checkout@v3"},
                grant_repo_access(),
                create_set_env_step(SAPLING_VERSION, "$(ci/tag-name.sh)"),
                {
                    "name": "Prepare build environment",
                    "run": "eden/scm/packaging/mac/prepare_environment.py \\\n"
                    + f"-s {openssl_bottle_hash} -f openssl@1.1 \\\n"
                    + f"-s {python_bottle_hash} -f python@3.11 \\\n"
                    + f"-t {target} \\\n"
                    + "-r ${{ env.SAPLING_VERSION }} \\\n"
                    + "-o $(brew tap-info homebrew/core | sed -n '2p' | awk '{printf $1}')/Formula/sapling.rb",
                },
                {
                    "name": "Install and build Sapling bottle",
                    "run": "brew install --build-bottle sapling",
                },
                {
                    "name": "Create Sapling bottle",
                    "run": "brew bottle sapling",
                },
                {
                    "name": "Rename bottle to some platform specific name",
                    "run": "mv %s sapling_${{ env.SAPLING_VERSION }}.%s"
                    % (artifact_glob, final_ext),
                },
                upload_artifact(artifact_key, artifact_glob),
            ],
        }

        publish_job = {
            "runs-on": "ubuntu-latest",
            "needs": BUILD,
            "steps": publish_release_steps(artifact_key, artifact_glob),
        }

        gh_action = {
            "name": f"Release - Homebrew macOS-{arch}",
            "on": release_trigger_on(),
            "jobs": {
                BUILD: build_job,
                "publish": publish_job,
            },
        }
        self._write_file(f"sapling-cli-homebrew-macos-{arch}-release.yml", gh_action)

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


def create_set_env_step(env_var: str, env_expr: str):
    """See https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions#setting-an-environment-variable"""
    return {
        "name": f"set-env {env_var}",
        # Note setting to shell to Bash is important when running on Windows.
        "shell": "bash",
        "run": f'echo "{env_var}={env_expr}" >> $GITHUB_ENV',
    }


def grant_repo_access():
    return {
        "name": "Grant Access",
        "run": 'git config --global --add safe.directory "$PWD"',
    }


def upload_artifact(name: str, path: str):
    return {
        "name": "Upload Artifact",
        "uses": "actions/upload-artifact@v3",
        "with": {
            "name": name,
            "path": path,
        },
    }


def release_trigger_on():
    return {
        "workflow_dispatch": None,
        "push": {"tags": ["v*", "test-release-*"]},
    }


def publish_release_steps(artifact_key: str, artifact_upload_glob: str):
    return [
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
            "run": f"bash ci/retry.sh gh release upload --clobber $(ci/tag-name.sh) {artifact_upload_glob}",
        },
    ]


def eprint(*args, **kwargs):
    print(*args, file=sys.stderr, **kwargs)


if __name__ == "__main__":
    main()
