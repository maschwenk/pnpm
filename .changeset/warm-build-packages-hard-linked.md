---
"pacquet": patch
"pnpm": patch
---

A repeat install from a warm store no longer copies every package that has a lifecycle script into `node_modules`. On Linux and macOS such a package is hard-linked like any other when its files come from the store, and pnpm gives it private copies of its files only right before its build actually runs. When the build comes from the side-effects cache, or `allowBuilds` denies it, nothing is copied. In an 8,000-package workspace this skipped several hundred MB of copying per install and cut a repeat install on Linux from 10.4 s to 8.4 s.
