---
"pacquet": patch
---

`pnpm install` in a workspace with `sharedWorkspaceLockfile: false` now keeps only a bounded number of project installs in memory at a time. A workspace with thousands of projects that depend on each other no longer runs out of memory [pnpm/pnpm#16966](https://github.com/pnpm/pnpm/issues/16966).
