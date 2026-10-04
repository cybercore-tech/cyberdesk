# Changelog

## Unreleased

### Added

- Cyberdesk now reads built-in and user-created themes from the shared
  Cybercore `ThemeCatalog`, and persists theme selection across Cybercore
  apps.
- Theme CSS includes the shared typography, density, corner, and motion
  design tokens, plus the currently selected dark/light palette variant.

### Changed

- Theme family and palette lookup no longer depend on a Cybercore source-tree
  theme directory being available at runtime.
