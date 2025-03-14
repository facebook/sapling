# Managing the EdenFS releases

## Build pipeline

The SCM repo manager used in the Remote Execution stack deploys EdenFS as part of the tupperware image: https://fburl.com/code/xkp2hiny.

This image is built and released as part of the [SCM RE agent Conveyor](https://www.internalfb.com/svc/services/remote_execution/scm_agent/conveyor/remote_execution/scm_agent/releases).

## Finding the EdenFS version

Before reverting, we should verify which version is currently running in prod.

This can be achieved by logging onto one of the RE agent tasks:
```
tw ssh tsp_cco/remote_execution/scm_agent.prod/0
```
Then we can check the RPM included in the currently running job:
```
[06:09:10 root@tsp_cco/remote_execution/scm_agent.prod/0 ~]$ rpm -q fb-eden
fb-eden-20250310-234332.x86_64
```

## Revert an EdenFS version

You can revert through the conveyor UI linked above. This will ensure the versions of the fbpkgs still exist and it will handle the push automatically.
To speed up the process, you can tell the revert to skip bake time.

## Conveyor UI guide

1. Go to the [conveyor]((https://www.internalfb.com/svc/services/remote_execution/scm_agent/conveyor/remote_execution/scm_agent/releases)) (bunnylol `conveyor remote_execution/scm_agent`)
2. Click on "Release History"
3. Click on the bundle in the timeline you would like to revert
    a. Bundles with green checkmarks in the "Push" node are the ones that have been promoted to production (i.e. release to all users).
    b. Bundles with a clock in the "Push" node are the ones that are currently scheduled for the next release
    c. Bundles with a play (solid blue triangle) sign are the ones that are currently being rolled out
4. Click on the prod node in the list of node runs.

![](px/6MqP4)

5. Click the revert button.
6. Click through the revert popup. You may also optionally want to use skip bake time in the revert popup, to make the revert faster.

### After reverting

Make sure the conveyor is disabled/blocked if this did not happen automatically.

### Checking the Most Recent Release

If you ever need to check what the most recent (completed or in-progress) release was, you can check via the Conveyor UI. Here are the steps to do this:
1. bunnylol `conveyor remote_execution/scm_agent`
2. The landing page has a lot of info. The content you're interested in is the "Last release" info and the "Running Release" info for the prod node. The latest bundle corresponds to the last completed release. The running corresponds to the current ongoing release

![](px/6Mr4R)

3. You can click on the "Running Release" or "Last Release" to figure out what image fbpkg is currently being released or was recently released. After clicking on the package, navigate to "Release Contents" on the lefthand side to view the package name.

![](px/6Mr6l)
