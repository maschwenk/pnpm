---
"pacquet": patch
"pnpm": patch
---

Cold installs on macOS are faster. APFS serializes metadata writes volume-wide, so the many threads pnpm used to write extracted package files into the store mostly waited on a kernel lock: on an 18-core Mac, a cold install of an 8,000-package workspace spent 1,122 s of system time over 276 s of wall time. pnpm now writes with at most 4 threads on macOS, which brought that install to 227 s. Set `PNPM_STORE_WRITE_CONCURRENCY` to choose a different limit on any platform.
