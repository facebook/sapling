#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

import dotslash

config = {
    "target": "fbsource//xplat/scm/ghstack-0.6.0:ghstack_dotslash_artifact",
    "file_name": "ghstack",
    "exec_path": "bin/ghstack",
}

dotslash.export_multi_platform_fbcode_build(
    platforms={
        "linux": config,
        "macos": config,
        # Is buck2 not available on Windows right now?
        # "windows": config,
    },
    generated_dotslash_file="xplat/js/tools/typescript/tsc",
    oncall="eden_oss",
    storage=dotslash.Storage.EVERSTORE,
)
