# Changelog

All notable changes to Kamal Desktop Manager are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Containers tab: app containers from every host in one tree, grouped by image version, with a Rollback button on
  old versions; accessories listed separately.
- Host cards with CPU, load, memory and disk per server, refreshed every 5 seconds.
- Runs tab: deploy, redeploy, roll back, lock and unlock with streamed output, and a history of every run with its
  git SHA, duration and log.
- Logs tab: follow any container with `--grep`, `--since` and a 10k-line buffer.
- Proxy tab: kamal-proxy routes, TLS status and live responses by status class.
- Console tab: an embedded terminal running `kamal app exec -i`.
- Signed in-app updates.

[Unreleased]: https://github.com/rslhdyt/kamal-desktop-manager/commits/main
