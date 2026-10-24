## What changed and why

<!-- Link the route/resolver or issue this relates to. -->

## Verification

<!-- Paste the commands you ran and their real result. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cd apps/web && npm run check`
- [ ] `docker compose up` smoke check, if migrations or routes changed

## Checklist

- [ ] API docs and clients updated for any contract change
- [ ] No secrets, tokens, or tenant data committed
- [ ] README/docs updated if coverage or limitations changed
