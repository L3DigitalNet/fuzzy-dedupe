# MS-0 CI gate evidence (EV-001)

GitHub Actions run logs expire, so this file keeps the pull-request gate results for MS-0. It proves the pull-request part of exit item 3 (REQ-020, PV-T5-001).

- Pull request: #9
- Verified head commit: `d5e15de03a512d1042940ae8bf39f39e12e10936`
- Date: 2026-11-22
- Source: `gh run list --commit <head> --json databaseId,workflowName,event,conclusion,headSha` and `gh run view <id> --json jobs`, restricted to runs whose `headSha` equals the verified head.

## Workflow runs on the verified head

| Workflow               | Run ID      | Event        | Conclusion |
| ---------------------- | ----------- | ------------ | ---------- |
| Rust                   | 45065943980 | pull_request | success    |
| REUSE                  | 45065943976 | pull_request | success    |
| Format                 | 45065944382 | pull_request | success    |
| Lint Markdown          | 45065944379 | pull_request | success    |
| Validate project specs | 45065944257 | pull_request | success    |

## Jobs

| Workflow               | Run ID      | Job                      | Conclusion |
| ---------------------- | ----------- | ------------------------ | ---------- |
| Rust                   | 45065943980 | fmt                      | success    |
| Rust                   | 45065943980 | clippy                   | success    |
| Rust                   | 45065943980 | test                     | success    |
| Rust                   | 45065943980 | deny                     | success    |
| Rust                   | 45065943980 | audit                    | success    |
| Rust                   | 45065943980 | portability              | success    |
| Rust                   | 45065943980 | boundaries               | success    |
| REUSE                  | 45065943976 | reuse                    | success    |
| Format                 | 45065944382 | format / Prettier        | success    |
| Lint Markdown          | 45065944379 | lint-markdown / Markdown | success    |
| Validate project specs | 45065944257 | validate-specs / Specs   | success    |

All seven `rust.yml` jobs, the `reuse` job and every job of the `format`, `lint-markdown` and `validate-specs` workflows are present and succeeded.

The `admission` workflow is absent by design: it runs only on pushes to `main`, so it never runs on a pull-request head.

## Gate tool versions

| Tool | Version | Source |
| --- | --- | --- |
| Rust toolchain | 1.96.0 | `rust-toolchain.toml` |
| cargo-nextest | 0.9.152 | `.rexec.toml` |
| cargo-deny | 0.22.0 | `.rexec.toml` |
| cargo-audit | 0.23.0 | `.rexec.toml` |
| reuse | 6.3.0 | `.github/workflows/reuse.yml` |
| actions/checkout | `3d3c42e5aac5ba805825da76410c181273ba90b1` (v7) | `.github/workflows/*.yml` |

Release assets and SHA-256 digests, as pinned in `.rexec.toml` and installed by `rust.yml`:

| Tool | Asset | SHA-256 |
| --- | --- | --- |
| cargo-nextest | `cargo-nextest-0.9.152-x86_64-unknown-linux-gnu.tar.gz` | `d039b43c3e0958441b6ad0a39c8884d018b3d2e8fb9928f209074b1100b5e17a` |
| cargo-deny | `cargo-deny-0.22.0-x86_64-unknown-linux-musl.tar.gz` | `98c56e6a408bfa5c3c60744b6015aaa6aa08f743fdeb88386d06fca035bb1786` |
| cargo-audit | `cargo-audit-x86_64-unknown-linux-gnu-v0.23.0.tgz` | `b77993532b253fcbf550996d7716465ec3d734d4e74e7f87bfb65a19add0fb59` |
