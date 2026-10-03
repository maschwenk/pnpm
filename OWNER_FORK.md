# owner fork notes

branch `owner/cold-install-perf-v12.7` = pnpm v12.7.0 plus two install-speed changes. measured on the owner monorepo (7.9k packages, 179 projects).

## perf: cap concurrent store writers on macOS

apfs serializes metadata writes volume-wide. the tarball extraction pools default to one or two writers per core, so on an 18-core mac a cold install spends most of its time spinning in the kernel. this caps the cas-write pool and the post-download / streaming-extract permits at 4 on macos. `PNPM_STORE_WRITE_CONCURRENCY` overrides it on any platform.

cold install (empty store, no node_modules), macos, 18 cores:

| | wall | store-filling phase | system time |
|-|-|-|-|
| 12.7.0 | 276s | 241s | 1122s |
| fork | 227s | 194s | 798s |

on 12.1.0 with the global virtual store on, the same cap took the store-filling phase from 272s to 168s.

## perf: keep hard links for warm build packages, unshare them when a build runs

12.7.0 imports every package with a lifecycle script or patch via clone-or-copy so a build can't write through a hard link into the store. on a warm reinstall the build usually comes from the side-effects cache, so that copy is wasted (aws-sdk, core-js, onnxruntime-node, temporal core-bridge, react-native-skia: several hundred mb per install). on unix, warm slots now keep hard links and the build phase gives a slot private inodes right before its scripts run. freshly downloaded slots still copy at import.

warm store, no node_modules, linux arm64: 12.7.0 10.4s, fork 8.4s (mean of 3). cold is unchanged (38.7s vs 39.1s).

## building

```sh
mv .cargo/config.toml /tmp/   # the repo vendors crates under .pnpm/crates via pnpm; cargo fetches them instead
cargo build --release -p pnpm-cli --bin pnpm
mv /tmp/config.toml .cargo/
```
