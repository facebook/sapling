# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# E2E integration test for multi-repo landing through git server with 5 code repos
# (2 LFS-enabled) and 1 manifest repo. Each push is diverted through the mock RL
# Land Service, which updates the manifest and atomically moves bookmarks.
#
# Flow:
#   1. Git client pushes to an AOSP repo via the Git server
#   2. Git server detects "aosp/" prefix + JK enabled, diverts to Mock RL Land Service
#   3. Mock RL Land Service fetches current manifest, updates it, forwards to backend
#   4. Multi-Repo Land Service atomically moves bookmarks + updates manifest
#   5. Test verifies: all bookmarks moved, manifest has all new git SHAs

  $ . "${TEST_FIXTURES}/library.sh"

Configure the RL Land Service repo prefix
  $ export ADDITIONAL_MONONOKE_COMMON_CONFIG='rl_land_service_repo_prefix="aosp/"'

-- Setup repos --

Setup 3 regular code repos (names start with "aosp/" to trigger diversion)
  $ REPOID=0 REPONAME="aosp/platform_build" setup_common_config blob_files
  $ REPOID=1 REPONAME="aosp/platform_frameworks" setup_common_config blob_files
  $ REPOID=2 REPONAME="aosp/platform_packages" setup_common_config blob_files

Setup 2 LFS-enabled code repos (LFS group membership is tracked in the manifest)
  $ REPOID=3 REPONAME="aosp/device_meta_common" setup_common_config blob_files
  $ REPOID=4 REPONAME="aosp/device_meta_stanley" setup_common_config blob_files

Setup manifest repo
  $ REPOID=5 REPONAME=manifest_repo setup_common_config blob_files

-- Populate code repos --

Create and import aosp/platform_build
  $ GIT_REPO_1="${TESTTMP}/git_repo_1"
  $ mkdir -p "$GIT_REPO_1" && cd "$GIT_REPO_1"
  $ git init -q
  $ echo "build system config" > build.mk
  $ git add build.mk
  $ git commit -qam "Initial build config"
  $ INITIAL_SHA_1=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ REPOID=0 quiet gitimport "$GIT_REPO_1" --derive-hg --generate-bookmarks full-repo
  $ REPO_ID=0 REPONAME="aosp/platform_build" set_mononoke_as_source_of_truth_for_git

Create and import aosp/platform_frameworks
  $ GIT_REPO_2="${TESTTMP}/git_repo_2"
  $ mkdir -p "$GIT_REPO_2" && cd "$GIT_REPO_2"
  $ git init -q
  $ echo "framework base code" > Framework.java
  $ git add Framework.java
  $ git commit -qam "Initial framework code"
  $ INITIAL_SHA_2=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ REPOID=1 quiet gitimport "$GIT_REPO_2" --derive-hg --generate-bookmarks full-repo
  $ REPO_ID=1 REPONAME="aosp/platform_frameworks" set_mononoke_as_source_of_truth_for_git

Create and import aosp/platform_packages
  $ GIT_REPO_3="${TESTTMP}/git_repo_3"
  $ mkdir -p "$GIT_REPO_3" && cd "$GIT_REPO_3"
  $ git init -q
  $ echo "package manager code" > PackageManager.java
  $ git add PackageManager.java
  $ git commit -qam "Initial package manager"
  $ INITIAL_SHA_3=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ REPOID=2 quiet gitimport "$GIT_REPO_3" --derive-hg --generate-bookmarks full-repo
  $ REPO_ID=2 REPONAME="aosp/platform_packages" set_mononoke_as_source_of_truth_for_git

Create and import aosp/device_meta_common (LFS-enabled)
  $ GIT_REPO_4="${TESTTMP}/git_repo_4"
  $ mkdir -p "$GIT_REPO_4" && cd "$GIT_REPO_4"
  $ git init -q
  $ echo "common device config" > device.mk
  $ git add device.mk
  $ git commit -qam "Initial common device config"
  $ INITIAL_SHA_4=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ REPOID=3 quiet gitimport "$GIT_REPO_4" --derive-hg --generate-bookmarks full-repo
  $ REPO_ID=3 REPONAME="aosp/device_meta_common" set_mononoke_as_source_of_truth_for_git

Create and import aosp/device_meta_stanley (LFS-enabled)
  $ GIT_REPO_5="${TESTTMP}/git_repo_5"
  $ mkdir -p "$GIT_REPO_5" && cd "$GIT_REPO_5"
  $ git init -q
  $ echo "stanley device config" > stanley.mk
  $ git add stanley.mk
  $ git commit -qam "Initial stanley device config"
  $ INITIAL_SHA_5=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ REPOID=4 quiet gitimport "$GIT_REPO_5" --derive-hg --generate-bookmarks full-repo
  $ REPO_ID=4 REPONAME="aosp/device_meta_stanley" set_mononoke_as_source_of_truth_for_git

-- Populate manifest repo --

Create initial manifest.xml referencing all 5 repos with their initial git SHAs
  $ testtool_drawdag -R manifest_repo --derive-all << EOF
  > M1
  > # bookmark: M1 master_bookmark
  > # modify: M1 manifest.xml "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest>\n  <remote name=\"origin\" fetch=\"..\"/>\n  <default remote=\"origin\" sync-j=\"12\" sync-c=\"true\" sync-tags=\"false\" revision=\"master_bookmark\"/>\n  <project name=\"aosp/platform_build\" path=\"platform/build\" revision=\"$INITIAL_SHA_1\" upstream=\"master_bookmark\"/>\n  <project name=\"aosp/platform_frameworks\" path=\"platform/frameworks\" revision=\"$INITIAL_SHA_2\" upstream=\"master_bookmark\"/>\n  <project name=\"aosp/platform_packages\" path=\"platform/packages\" revision=\"$INITIAL_SHA_3\" upstream=\"master_bookmark\"/>\n  <project name=\"aosp/device_meta_common\" path=\"device/meta/common\" revision=\"$INITIAL_SHA_4\" upstream=\"master_bookmark\" groups=\"lfs,meta\"/>\n  <project name=\"aosp/device_meta_stanley\" path=\"device/meta/stanley\" revision=\"$INITIAL_SHA_5\" upstream=\"master_bookmark\" groups=\"lfs,meta\"/>\n</manifest>\n"
  > EOF
  M1=* (glob)

-- Start services --

Enable the diversion JustKnob
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:divert_aosp_push_to_rl_land_service": true
  >   }
  > }
  > EOF

Start the Multi-Repo Land Service (backend)
  $ start_and_wait_for_multi_repo_land_service

Start the Mock RL Land Service (proxy that updates manifest)
  $ start_and_wait_for_mock_rl_land_service \
  >   --manifest-repo manifest_repo \
  >   --manifest-bookmark master_bookmark \
  >   --manifest-path manifest.xml

Start the Git server pointing to the Mock RL Land Service
  $ mononoke_git_service --multi-repo-land-service-address "$(mock_rl_land_service_address)"

-- Push to each repo via git server --

Push to aosp/platform_build
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/platform_build.git" clone_1
  $ cd clone_1
  $ echo "new build rule for target X" > new_build.mk
  $ git add new_build.mk
  $ git commit -qam "Add new build rule"
  $ NEW_SHA_1=$(git rev-parse HEAD)
  $ git_client push origin master_bookmark
  To https://*/repos/git/ro/aosp/platform_build.git (glob)
     *..*  master_bookmark -> master_bookmark (glob)
  $ wait_for_git_bookmark_move HEAD "$INITIAL_SHA_1"

Push to aosp/platform_frameworks
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/platform_frameworks.git" clone_2
  $ cd clone_2
  $ echo "new framework feature" > NewFeature.java
  $ git add NewFeature.java
  $ git commit -qam "Add new framework feature"
  $ NEW_SHA_2=$(git rev-parse HEAD)
  $ git_client push origin master_bookmark
  To https://*/repos/git/ro/aosp/platform_frameworks.git (glob)
     *..*  master_bookmark -> master_bookmark (glob)
  $ wait_for_git_bookmark_move HEAD "$INITIAL_SHA_2"

Push to aosp/platform_packages
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/platform_packages.git" clone_3
  $ cd clone_3
  $ echo "new package installer" > Installer.java
  $ git add Installer.java
  $ git commit -qam "Add new package installer"
  $ NEW_SHA_3=$(git rev-parse HEAD)
  $ git_client push origin master_bookmark
  To https://*/repos/git/ro/aosp/platform_packages.git (glob)
     *..*  master_bookmark -> master_bookmark (glob)
  $ wait_for_git_bookmark_move HEAD "$INITIAL_SHA_3"

Push to aosp/device_meta_common (LFS-enabled)
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/device_meta_common.git" clone_4
  $ cd clone_4
  $ echo "updated common overlay" > overlay.mk
  $ git add overlay.mk
  $ git commit -qam "Add common overlay config"
  $ NEW_SHA_4=$(git rev-parse HEAD)
  $ git_client push origin master_bookmark
  To https://*/repos/git/ro/aosp/device_meta_common.git (glob)
     *..*  master_bookmark -> master_bookmark (glob)
  $ wait_for_git_bookmark_move HEAD "$INITIAL_SHA_4"

Push to aosp/device_meta_stanley (LFS-enabled)
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/device_meta_stanley.git" clone_5
  $ cd clone_5
  $ echo "stanley sensor calibration" > sensors.mk
  $ git add sensors.mk
  $ git commit -qam "Add stanley sensor config"
  $ NEW_SHA_5=$(git rev-parse HEAD)
  $ git_client push origin master_bookmark
  To https://*/repos/git/ro/aosp/device_meta_stanley.git (glob)
     *..*  master_bookmark -> master_bookmark (glob)
  $ wait_for_git_bookmark_move HEAD "$INITIAL_SHA_5"

-- Verify: push diversion --

All 5 pushes were diverted through the git server
  $ grep -c "Diverting push for repo" "$TESTTMP/mononoke_git_service.out"
  5

Mock RL Land Service received all 5 requests
  $ grep -c "Mock RL Land Service received" "$TESTTMP/mock_rl_land_service.out"
  5

Mock RL Land Service forwarded all 5 enriched requests
  $ grep -c "Forwarding enriched request to backend" "$TESTTMP/mock_rl_land_service.out"
  5

-- Verify: repo content via fresh clones --

Verify aosp/platform_build has new content
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/platform_build.git" verify_1
  $ cd verify_1 && test -f new_build.mk && echo "new_build.mk exists"
  new_build.mk exists

Verify aosp/platform_frameworks has new content
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/platform_frameworks.git" verify_2
  $ cd verify_2 && test -f NewFeature.java && echo "NewFeature.java exists"
  NewFeature.java exists

Verify aosp/platform_packages has new content
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/platform_packages.git" verify_3
  $ cd verify_3 && test -f Installer.java && echo "Installer.java exists"
  Installer.java exists

Verify aosp/device_meta_common has new content
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/device_meta_common.git" verify_4
  $ cd verify_4 && test -f overlay.mk && echo "overlay.mk exists"
  overlay.mk exists

Verify aosp/device_meta_stanley has new content
  $ cd "$TESTTMP"
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/device_meta_stanley.git" verify_5
  $ cd verify_5 && test -f sensors.mk && echo "sensors.mk exists"
  sensors.mk exists

-- Verify: manifest updated with all new SHAs --

Fetch final manifest content and verify all 5 repos have updated revision hashes
  $ MANIFEST_CONTENT=$(multi_repo_land_service_client fetch-manifest-content -R manifest_repo -B master_bookmark --path manifest.xml 2>/dev/null)
  $ echo "$MANIFEST_CONTENT" | grep -c "revision=\"$NEW_SHA_1\""
  1
  $ echo "$MANIFEST_CONTENT" | grep -c "revision=\"$NEW_SHA_2\""
  1
  $ echo "$MANIFEST_CONTENT" | grep -c "revision=\"$NEW_SHA_3\""
  1
  $ echo "$MANIFEST_CONTENT" | grep -c "revision=\"$NEW_SHA_4\""
  1
  $ echo "$MANIFEST_CONTENT" | grep -c "revision=\"$NEW_SHA_5\""
  1

Verify LFS-enabled repos still have their groups attribute in the manifest
  $ echo "$MANIFEST_CONTENT" | grep "device_meta_common" | grep -c "groups=\"lfs,meta\""
  1
  $ echo "$MANIFEST_CONTENT" | grep "device_meta_stanley" | grep -c "groups=\"lfs,meta\""
  1

Verify manifest repo bookmark has moved from the original commit
  $ start_and_wait_for_scs_server
  $ MANIFEST_HEAD=$(scsc lookup -R manifest_repo -B master_bookmark -S bonsai 2>/dev/null)
  $ [[ "$MANIFEST_HEAD" != "$M1" ]] && echo "manifest bookmark moved to new commit"
  manifest bookmark moved to new commit

-- Verify: gclone git subcommand --

Configure git SSL for gclone (gclone spawns git clone as a subprocess, inherits env)
  $ git config --global http.sslCAInfo "$TEST_CERTDIR/root-ca.crt"
  $ git config --global http.sslCert "$TEST_CERTDIR/client0.crt"
  $ git config --global http.sslKey "$TEST_CERTDIR/client0.key"

Test gclone git on aosp/platform_build
  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/platform_build.git" gclone_git_1 -b master_bookmark --partial-clone false
  $ test -f gclone_git_1/new_build.mk && echo "gclone git: new_build.mk exists"
  gclone git: new_build.mk exists

Test gclone git on aosp/platform_frameworks
  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/platform_frameworks.git" gclone_git_2 -b master_bookmark --partial-clone false
  $ test -f gclone_git_2/NewFeature.java && echo "gclone git: NewFeature.java exists"
  gclone git: NewFeature.java exists

Test gclone git on aosp/platform_packages
  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/platform_packages.git" gclone_git_3 -b master_bookmark --partial-clone false
  $ test -f gclone_git_3/Installer.java && echo "gclone git: Installer.java exists"
  gclone git: Installer.java exists

Test gclone git on aosp/device_meta_common
  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/device_meta_common.git" gclone_git_4 -b master_bookmark --partial-clone false
  $ test -f gclone_git_4/overlay.mk && echo "gclone git: overlay.mk exists"
  gclone git: overlay.mk exists

Test gclone git on aosp/device_meta_stanley
  $ cd "$TESTTMP"
  $ quiet "$GCLONE" git "$MONONOKE_GIT_SERVICE_BASE_URL/aosp/device_meta_stanley.git" gclone_git_5 -b master_bookmark --partial-clone false
  $ test -f gclone_git_5/sensors.mk && echo "gclone git: sensors.mk exists"
  gclone git: sensors.mk exists
