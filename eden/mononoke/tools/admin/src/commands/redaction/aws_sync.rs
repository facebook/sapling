/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use mononoke_types::typed_hash::RedactionKeyListId;
use tokio::process::Command;

const EKS_CONFIG_TIMEOUT: Duration = Duration::from_secs(30);
const POD_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const KUBECTL_EXEC_TIMEOUT: Duration = Duration::from_secs(60);

/// Build the kubectl command args to discover a running Mononoke pod.
fn build_discover_cmd() -> Vec<String> {
    vec![
        "get".to_string(),
        "pods".to_string(),
        "-l".to_string(),
        "app.kubernetes.io/name=mononoke-server".to_string(),
        "--field-selector=status.phase=Running".to_string(),
        "--request-timeout=30s".to_string(),
        "-o".to_string(),
        "jsonpath={.items[0].metadata.name}".to_string(),
    ]
}

/// Build the kubectl exec command args to run create-key-list-from-ids on the AWS pod.
fn build_exec_cmd(pod_name: &str, shadow_repo: &str, keys: &[String]) -> Vec<String> {
    let mut args = vec![
        "exec".to_string(),
        pod_name.to_string(),
        "-c".to_string(),
        "server".to_string(),
        "--request-timeout=60s".to_string(),
        "--".to_string(),
        "monad".to_string(),
        "redaction".to_string(),
        "create-key-list-from-ids".to_string(),
        "-R".to_string(),
        shadow_repo.to_string(),
        "--skip-aws-sync".to_string(),
    ];
    args.extend(keys.iter().cloned());
    args
}

/// Try to parse a RedactionKeyListId from kubectl exec stdout.
/// Returns true if the expected ID is found in the output.
fn output_contains_key_list_id(output: &str, expected_id: &RedactionKeyListId) -> bool {
    output.contains(&expected_id.to_string())
}

async fn ensure_eks_kubeconfig() -> Result<(), String> {
    let output = tokio::time::timeout(
        EKS_CONFIG_TIMEOUT,
        Command::new("cloud")
            .args([
                "eks",
                "update-kubeconfig",
                "mononoke-cloud",
                "us-west-2",
                "mononoke-prod",
            ])
            .output(),
    )
    .await
    .map_err(|_| "cloud eks update-kubeconfig timed out".to_string())?
    .map_err(|e| format!("Failed to run cloud CLI: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cloud eks update-kubeconfig failed: {stderr}"));
    }

    Ok(())
}

async fn discover_aws_pod() -> Result<String, String> {
    ensure_eks_kubeconfig().await?;

    let cmd_args = build_discover_cmd();
    let output = tokio::time::timeout(
        POD_DISCOVERY_TIMEOUT,
        Command::new("kubectl").args(&cmd_args).output(),
    )
    .await
    .map_err(|_| "kubectl pod discovery timed out".to_string())?
    .map_err(|e| format!("Failed to run kubectl: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("kubectl get pods failed: {stderr}"));
    }

    let pod_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if pod_name.is_empty() {
        return Err("No running Mononoke pods found on AWS".to_string());
    }

    Ok(pod_name)
}

fn build_manual_instructions(keys: &[String], shadow_repo: &str) -> String {
    let keys_str = keys.join(" ");
    format!(
        concat!(
            "To sync manually, run:\n",
            "  cloud eks update-kubeconfig mononoke-cloud us-west-2 mononoke-prod\n",
            "  kubectl get pods\n",
            "  kubectl exec <POD> -c server -- monad redaction create-key-list-from-ids -R {} {}",
        ),
        shadow_repo, keys_str,
    )
}

fn is_repo_not_found(stderr: &str, stdout: &str) -> bool {
    stderr.contains("not found") || stderr.contains("unknown repo") || stdout.contains("not found")
}

pub async fn sync_to_aws(keys: &[String], key_list_id: RedactionKeyListId, repo_name: &str) {
    let shadow_repo = format!("{repo_name}_shadow");
    eprintln!("\nChecking if sync to AWS is required...");

    let pod_name = match discover_aws_pod().await {
        Ok(pod) => {
            eprintln!("  → Discovered pod: {pod}");
            pod
        }
        Err(e) => {
            eprintln!("  → Warning: Failed to discover AWS pod ({e})");
            eprintln!("  → {}", build_manual_instructions(keys, &shadow_repo));
            return;
        }
    };

    eprintln!("  → Syncing keylist to {shadow_repo} on AWS...");

    let cmd_args = build_exec_cmd(&pod_name, &shadow_repo, keys);

    let result = tokio::time::timeout(
        KUBECTL_EXEC_TIMEOUT,
        Command::new("kubectl").args(&cmd_args).output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if output_contains_key_list_id(&stdout, &key_list_id) {
                eprintln!("  → AWS sync complete (ID matches)");
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if is_repo_not_found(&stderr, &stdout) {
                    eprintln!("  → No AWS shadow repo '{shadow_repo}' found. Skipping sync.");
                } else {
                    eprintln!("  → Warning: AWS sync completed but could not verify ID match");
                    eprintln!("  → Remote output: {}", stdout.trim());
                    eprintln!("  → Expected ID: {key_list_id}");
                }
            }
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            if is_repo_not_found(&stderr, &stdout) {
                eprintln!("  → No AWS shadow repo '{shadow_repo}' found. Skipping sync.");
            } else {
                eprintln!("  → Warning: kubectl exec failed: {}", stderr.trim());
                eprintln!("  → {}", build_manual_instructions(keys, &shadow_repo));
            }
        }
        Ok(Err(e)) => {
            eprintln!("  → Warning: Failed to run kubectl: {e}");
            eprintln!("  → {}", build_manual_instructions(keys, &shadow_repo));
        }
        Err(_) => {
            eprintln!("  → Warning: kubectl exec timed out after {KUBECTL_EXEC_TIMEOUT:?}");
            eprintln!("  → {}", build_manual_instructions(keys, &shadow_repo));
        }
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_shadow_repo_name() {
        assert_eq!(format!("{}_shadow", "fbsource"), "fbsource_shadow");
        assert_eq!(format!("{}_shadow", "opsfiles"), "opsfiles_shadow");
        assert_eq!(format!("{}_shadow", "admin"), "admin_shadow");
    }

    #[mononoke::test]
    fn test_manual_instructions_contains_repo_and_keys() {
        let keys = vec!["key1".to_string(), "key2".to_string()];
        let instructions = build_manual_instructions(&keys, "fbsource_shadow");
        assert!(instructions.contains("fbsource_shadow"));
        assert!(instructions.contains("key1 key2"));
        assert!(instructions.contains("cloud eks update-kubeconfig"));
        assert!(instructions.contains("kubectl exec"));
    }

    #[mononoke::test]
    fn test_build_discover_cmd_includes_label_and_timeout() {
        let args = build_discover_cmd();
        assert!(args.contains(&"get".to_string()));
        assert!(args.contains(&"pods".to_string()));
        assert!(args.contains(&"app.kubernetes.io/name=mononoke-server".to_string()));
        assert!(args.contains(&"--request-timeout=30s".to_string()));
        assert!(args.contains(&"jsonpath={.items[0].metadata.name}".to_string()));
    }

    #[mononoke::test]
    fn test_build_exec_cmd_structure() {
        let keys = vec!["key1".to_string(), "key2".to_string()];
        let args = build_exec_cmd("mononoke-pod-abc", "fbsource_shadow", &keys);

        assert_eq!(args[0], "exec");
        assert_eq!(args[1], "mononoke-pod-abc");
        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"server".to_string()));
        assert!(args.contains(&"--request-timeout=60s".to_string()));
        assert!(args.contains(&"--".to_string()));
        assert!(args.contains(&"monad".to_string()));
        assert!(args.contains(&"create-key-list-from-ids".to_string()));
        assert!(args.contains(&"-R".to_string()));
        assert!(args.contains(&"fbsource_shadow".to_string()));
        assert!(args.contains(&"--skip-aws-sync".to_string()));
        assert!(args.contains(&"key1".to_string()));
        assert!(args.contains(&"key2".to_string()));
    }

    #[mononoke::test]
    fn test_build_exec_cmd_keys_come_last() {
        let keys = vec!["keyA".to_string(), "keyB".to_string()];
        let args = build_exec_cmd("pod", "repo_shadow", &keys);
        let len = args.len();
        assert_eq!(args[len - 2], "keyA");
        assert_eq!(args[len - 1], "keyB");
    }

    #[mononoke::test]
    fn test_is_repo_not_found() {
        assert!(is_repo_not_found("repo not found", ""));
        assert!(is_repo_not_found("unknown repo xyz", ""));
        assert!(is_repo_not_found("", "not found"));
        assert!(!is_repo_not_found("", "success"));
        assert!(!is_repo_not_found("some other error", "ok"));
    }
}
