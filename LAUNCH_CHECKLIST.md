# Launch Checklist — Open Island

What must be done before any public communication (Show HN, Reddit, Twitter, etc.)

---

## Critical blockers (can't share without these)

- [ ] **1. Create a `LICENSE` file** — README says MIT but no actual license file exists. This is the #1 blocker. Without it, nobody can legally use your code.

- [ ] **2. Fix `tauri.conf.json`** — `beforeDevCommand` and `beforeBuildCommand` are still `"echo skip"` (temporary hack). Restore them to `"pnpm dev"` / `"pnpm build"` or nobody can build the project after cloning.

- [ ] **3. Add a screenshot** — README references `docs/screenshot.png` which doesn't exist. You have screenshots in `design/` — pick one, crop it, put it in `docs/`. A GIF would be even better for Show HN.

- [ ] **4. Fix README clone URL** — Still says `YOUR_USERNAME`. Replace with `SylvesterStaline1`.

- [ ] **5. Commit all pending changes** — 23 modified files and 20 new untracked files. Dirty working tree is a bad first impression.

- [ ] **6. Remove hook debug log** — `%TEMP%\oi-hook-log.txt` is an open debug artifact. Gate it behind `#[cfg(debug_assertions)]` or remove it entirely before release.

---

## Strongly advised (first impression matters)

- [ ] **7. Test Allow/Deny end-to-end once** — The CLAUDE.md says this is "UNTESTED". Verify it works with at least one real Claude Code session before anyone else tries it.

- [ ] **8. Delete or rewrite `STRATEGY.md`** — It's full of $19 commercialization plans and competitor analysis. Either remove it from the repo or repurpose it into a `ROADMAP.md` focused on open source.

- [ ] **9. Rethink the name** — `open-island-linux` is confusing when the project is now primarily Windows. Consider renaming the repo to just `open-island` or choosing the final name now. Easier to do before people start linking to it.

- [ ] **10. Verify `cargo tauri build` succeeds** — With the `echo skip` hack, no one has confirmed a release build actually produces a working binary.

---

## Nice to have (Show HN / Product Hunt polish)

- [ ] **11. Record a 15-20s demo GIF** — Terminal → pill appears → tool call pops up → click Allow → command runs. This is the single most important asset for Show HN and Reddit.

- [ ] **12. A basic GitHub Actions CI** — Just `cargo build` + `cargo test` on push. Shows the project is alive and maintained.

- [ ] **13. Clean up hook log properly** — Add a `--debug` flag to the hook binary, don't log by default.
