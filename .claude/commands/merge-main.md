1. Commit any changes
2. Fetch the latest main
3. Compare changes with yours and plan how you will merge.
4. Merge changes (for `Cargo.lock` conflicts, take either side and let `cargo build` regenerate it)
5. Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`
6. Fix issues
7. Commit and Push
