#!/bin/bash

cat <<EOF
{
  "trigger_spec": {
    "script": {
      "build_script": "fbcode/eden/addons/isl/release/dotslash_build.py",
      "sandcastle_builds": [
        {
          "msdk_output_dir": "universal",
          "override_capabilities": {
            "type": "fbcode",
            "vcs": "fbcode-fbsource",
            "marker": "eden"
          }
        }
      ]
    }
  },
  "bundle_spec": {
    "chunks": [
      {
        "path_root": "universal",
        "tags": {
          "InstallTarget": "linux",
          "Executable": "run-isl"
        }
      },
      {
        "path_root": "universal",
        "tags": {
          "InstallTarget": "macos",
          "Executable": "run-isl"
        }
      },
      {
        "path_root": "universal",
        "tags": {
          "InstallTarget": "windows",
          "Executable": "run-isl.bat"
        }
      }
    ],
    "storage_type": "cas"
  },
  "commit_spec": {
    "dotslash": {
      "executables_per_file": {
        "isl": ["run-isl"]
      }
    }
  }
}
EOF
