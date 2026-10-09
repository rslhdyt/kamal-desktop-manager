# Changelog

All notable changes to Kamal Desktop Manager are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-10-09

### Added

- Secrets setup: a "Secrets" button (and "Set up secrets" on a missing-secret error) lists the secrets the deploy config
  uses, marks those already in `.kamal/secrets*`, and generates `kamal secrets fetch` lines for any Kamal password
  manager adapter (1Password, Bitwarden, LastPass, AWS, GCP, Doppler, Enpass, Passbolt…) or the environment, to copy
  into the file. "Check" re-reads your shell environment and reloads the config.
- Console tab runs your deploy config's `aliases` (such as `shell` or `dbc`), so apps that don't use Rails get a
  console too.

### Changed

- The Rails console is offered only for apps that have `bin/rails`.

## [0.1.0] - 2026-10-04

### Added

- Projects: add any app folder with a Kamal `config/deploy.yml` and switch between its destinations.
- Containers tab: app containers from every host in one tree, grouped by image version, with a Rollback button on
  old versions; accessories listed separately.
- Host cards with CPU, load, memory and disk per server, refreshed every 5 seconds.
- Runs tab: deploy, redeploy, roll back, lock and unlock with streamed output and a Cancel button, and a history of
  every run with its git SHA, duration and log.
- Logs tab: follow any container with `--grep`, `--since` and a 10k-line buffer.
- Proxy tab: kamal-proxy routes, TLS status and live responses by status class.
- Console tab: an embedded terminal running `kamal app exec -i`.
- Signed in-app updates.

[Unreleased]: https://github.com/rslhdyt/kamal-desktop-manager/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/rslhdyt/kamal-desktop-manager/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/rslhdyt/kamal-desktop-manager/tree/v0.1.0
