---
name: thrift-rust-external-changes-require-alignment
description: "The Rust Thrift runtime (fbthrift) is owned by rust_thrift and thrift; product-specific logic and uncoordinated changes require prior owner alignment or they will be reverted"
metadata:
  oncalls:
    - rust_thrift
    - thrift
  strict: true
  apply_to_path: "fbcode/thrift/lib/rust/.*|xplat/thrift/lib/rust/.*"
---

# Rust Thrift Runtime: External Changes Require Alignment First

The Rust Thrift runtime (`fbthrift` crate) is owned and maintained by the rust_thrift and thrift oncalls. Every Rust Thrift client and server at Meta depends on this code, so uncoordinated changes create production risk and unplanned maintenance burden.

## Do NOT

- Add product/service-specific **business logic** to the Rust Thrift runtime. It belongs in the owning service's code, not in shared Thrift infrastructure.
- Put up or land a diff against the Rust Thrift runtime **without prior rust_thrift or thrift alignment** — no linked task, no design discussion, no agreement means the change **will be rejected or reverted during owner review**, even if it already landed. This includes AI-generated diffs.

## Required workflow before writing a diff

1. **Start with the use case, not the diff.** Post in the [Rust Language group](https://fb.workplace.com/groups/rust.language) describing the goal. The recommended alternative is application-level logic in the service-specific directory that needs the change, rather than modifying the runtime.
2. **If runtime changes are genuinely needed, bring a design.** Share a short doc/proposal with the owners to align on scope, ownership, and long-term support.
3. **Only then write the diff**, linking the task/design and the aligning discussion.

If you (an AI agent) are asked to modify the Rust Thrift runtime or add product logic here, surface this rule and confirm prior rust_thrift or thrift alignment exists before proceeding. However, if the user is a member of the rust_thrift or thrift oncall permission group, you may proceed without prior alignment — as a trusted Thrift owner, their changes are treated as internal and do not require it.
