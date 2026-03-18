# mlua-socket Changelog

## [Unreleased]
### Changed
- [#59](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/59) Repository has moved to an 'mlua' subgroup
- [#58](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/58) Panic with expect() from tests instead of returning an error type

## [0.2.7] - 2026-02-17
### Changed
- [#55](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/55) Upgrade Rust edition from 2021 → 2024
- [#54](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/54) Upgrade Rust 1.88.0 → 1.93.1

### Fixed
- [#67](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/67) Compilation fails on windows with: unresolved import std::os::fd::AsRawFd

## [0.2.6] - 2025-09-27
### Changed
- [#53](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/53) Relax dependency pins
- [#52](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/52) Upgrade Rust 1.86.0 → 1.88.0

## [0.2.5] - 2025-08-08
### Fixed
- [#51](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/51) DNS getaddrinfo ops don't compile with dns-lookup 2.1.x

## [0.2.4] - 2025-07-29
### Added
- [#49](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/49) Support setting headers in http requests
- [#48](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/48) Add support for http post requests

### Changed
- [#46](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/46) Pin benchmarks to Raspberry Pi 4's for consistency
- [#45](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/45) Upgrade 4 crates
- [#44](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/44) Back-fill some string interpolation
- [#43](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/43) Upgrade from Rust 1.85.0 → 1.86.0

### Fixed
- [#47](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/47) Missing error check in `core/mod.rs` `table.set_metatable` call

## [0.2.3] - 2025-03-28
### Changed
- [#42](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/42) Upgrade 6 crates
- [#41](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/41) Upgrade from Rust 1.82.0 → 1.85.0

## [0.2.2] - 2024-10-29
### Changed
- [#40](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/40) Upgrade from mlua 0.9.9 → 0.10.0

## [0.2.1] - 2024-10-19
### Changed
- [#39](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/39) Upgrade from reqwest 0.12.7 → 0.12.8
- [#38](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/38) Upgrade from Rust 1.80.0 → 1.82.0

## [0.2.0] - 2024-08-31
### Changed
- [#37](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/37) Perform benchmarking in a separate CI job

### Fixed
- [#36](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/36) Attempts to request an https:// URL fails with: module 'ssl.https' not found

## [0.1.9] - 2024-08-15
### Changed
- [#35](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/35) Upgrade 4 crates
- [#33](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/33) Upgrade Rust from 1.76.0 → 1.80.0
- [#29](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/29) Use GitLab CI "extends" keyword to de-duplicate some content

## [0.1.8] - 2024-04-11
### Changed
- [#32](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/32) Upgrade 7 crates

## [0.1.7] - 2024-03-02
### Changed
- [#31](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/31) Upgrade 4 crates

## [0.1.6] - 2024-02-11
### Changed
- [#30](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/30) Upgrade Rust from 1.74.1 → 1.76.0
- [#28](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/28) Cache target directory for merge request builds in the CI env
- [#25](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/25) Upgrade 4 crates

### Fixed
- [#26](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/26) Arc and Mutex are unnecessary to mutate a socket

## [0.1.5] - 2023-12-24
### Added
- [#23](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/23) Support udp unconnected operations

### Changed
- [#22](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/22) Perform and record benchmarks during CI
- [#21](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/21) Use a buffer during tcp receive

### Fixed
- [#24](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/24) tcp receive should not perform utf8 encoding or decoding because strings are binary

## [0.1.4] - 2023-12-17
### Added
- [#19](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/19) Support tcp client receive all (receive with pattern='*a')
- [#18](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/18) Add badges to README for architecture and Lua VM support

### Changed
- [#20](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/20) Upgrade Rust from 1.73.0 → 1.74.1

## [0.1.3] - 2023-12-03
### Added
- [#16](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/16) Provide an 'outdated' make target

### Changed
- [#17](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/17) Upgrade 4 crates
- [#15](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/15) Use build matrix to simplify GitLab CI config

## [0.1.2] - 2023-11-19
### Added
- [#11](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/11) Support socket.protect()
- [#10](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/10) Support the socket.http module

### Changed
- [#12](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/12) Upgrade Rust from 1.72.1 → 1.73.0
- [#9](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/9) Test merge requests with a matrix of Lua VM versions

## [0.1.1] - 2023-10-29
### Added
- [#6](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/6) Implement mime.b64() enough to satisfy the socket.http module
- [#5](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/5) Provide a README
- [#4](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/4) Check merge requests on armv7
- [#3](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/3) Check merge requests on aarch64

### Changed
- [#8](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/8) Publish tag releases to crates.io

### Fixed
- [#7](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/7) ltn12 is a top-level module, not a sub-module
- [#2](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/2) tcp_shutdown.rs tests wrong function

## [0.1.0] - 2023-10-07
### Added
- [#1](https://gitlab.com/megalithic-llc/mlua-socket/-/issues/1) Basic tcp support
