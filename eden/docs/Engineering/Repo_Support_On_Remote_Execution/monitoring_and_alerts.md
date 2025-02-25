# Monitoring And Alerts

The ownership of the Source Control Management (SCM) on Remote Execution (RE) is clearly defined and divided between two teams:

* **The Source Control Team** is responsible for the [SCM Repo Manager Thrift Service](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/scm_repo_manager)
and the underlying "scm resources": EdenFS, Sapling, Watchman, Source Control configuration, and dependencies such as CASC or Mononoke.
* **The Remote Execution Team** is responsible for the `scm-repo-support` platform. It maintains the fleet of agents running the workers attached to the repo managers. It also receives the alerts for queues and exceptions. [Conveyor](https://www.internalfb.com/svc/services/remote_execution/scm_agent/conveyor/remote_execution/scm_agent/releases) should regularly push stable versions of the RE fbpkgs alongside newly built SCM fbpkgs.

## Source Control

The alerts on the Source Control side are directly derived from the SCM Repo Manager reliability [SLOs](https://www.internalfb.com/slick?service=scm%2Fscm_repo_manager&aggregation=DAY&heat_map_period=WEEK).

We are using the SLICK helpers to define the alerts (like `create_slo_burn_rate_detector`): [see alerts definitions](https://www.internalfb.com/code/configerator/source/scm/detectors/remote_execution_health.detector.cconf).

**Oncall Rotation**: [source_control](https://www.internalfb.com/omh/view/source_control)

## Remote Execution

Remote Execution [Platform SLO page](https://www.internalfb.com/slick?service=remote_execution%2Fremote_execution&tab=Details&aggregation=DAY&heat_map_period=WEEK&bucket=Global&sli=scm-repo-support%20reliability)

Remote Execution [Platform Health Unidash](https://fburl.com/unidash/lje68er5)

Remote Execution [Platform UI](https://fburl.com/remote_execution/shnouevp): `bunnylol replatform scm-repo-support`

Remote Execution Use Case UI: `bunnylol reusecase <NAME>` (e.g. https://fburl.com/remote_execution/k80b45ie)

**Oncall Rotation**: [remote_execution](https://www.internalfb.com/omh/view/remote_execution)


## What Oncall To Contact

Automated issue detection will trigger notifications to the relevant oncall team responsible for addressing the problem.

If error messages are unclear, customers should reach out to the [remote_execution](https://www.internalfb.com/omh/view/remote_execution) for initial issue triage.
However, if the error message clearly indicates the responsible team, customers should directly contact the corresponding oncall for assistance.
