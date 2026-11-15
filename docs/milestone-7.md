# Milestone 7: releases

PLAN.md row 7: "A friend installs from the GitHub release and reaches Steam without
help." State on 2026-11-15: built, awaiting the owner. The done-check needs a second
person and cannot be met by the lead.

## What exists

- `README.md`: what snisplit does, install, the window, the command line, adding a
  site, limits, antivirus, troubleshooting.
- `.github/workflows/release.yml`: on a manual run with a `version` input it checks
  the input against the workspace version, runs the tests in release mode, builds the
  workspace, and packs `snisplit.exe`, `snisplit-gui.exe`, `README.md` and both
  licences into `snisplit-<version>-windows-x86_64.zip` with a `.sha256` file. It
  then creates a draft release `v<version>` at the built commit. A push to a
  `ci/release*` branch runs the same build and uploads the archive as a workflow
  artifact without creating a release.
- Workspace version 0.1.0.

## Decisions

1. Draft only (D13). `gh release create --draft` makes no git tag; the tag `v0.1.0`
   appears when the owner publishes the draft.
2. One zip, no installer. Removing snisplit is deleting the folder; the per-user site
   folder `%APPDATA%\snisplit\sites` is the only other trace.
3. Unsigned. Signing and antivirus false-positive reports are owner decisions and
   were not started.
4. Windows x86_64 only; Linux and macOS archives wait for milestone 10.

## Checked

- Branch run 43619639294 (`ci/release-draft`, before the UI existed): archive built,
  no release created.
- Manual run on `main` with version 0.1.0 (run id and commit: see the draft release's
  target): tests and build passed; the archive holds `snisplit.exe`,
  `snisplit-gui.exe`, `README.md`, `LICENSE-MIT`, `LICENSE-APACHE`; the draft release
  `snisplit 0.1.0` exists; `git ls-remote --tags` shows no tag.
- The final window build on this PC, not elevated: the first frame paints with no
  input; a close 250 ms after pressing Start exits with code 0 after 837 ms, with no
  process and no rule left.

## Not checked

- Nobody downloaded the archive and ran it. The binaries CI built differ from the
  ones built on this PC, so an antivirus may treat them differently.
- SmartScreen behaviour on a downloaded, unsigned executable.

## Version 0.2.0 draft, 2026-11-15

The Release workflow built version 0.2.0 at commit 3b4afc4 and created the draft
release `v0.2.0`; no tag exists. The antivirus on this PC flagged `snisplit-gui.exe`
inside the downloaded archive about a minute after a clean first scan, while
`snisplit.exe` and the window binary of the v0.1.0 draft stayed clean. The owner
decided not to publish the 0.2.0 draft and to leave both drafts as they are; the tool
is used from local builds for now. Details: docs/window-redesign.md, last section.

Publish rule added: rescan the downloaded archive some minutes after download, not
only straight away, before publishing any draft. A clean first pass is weak evidence.

## Awaiting the owner

1. Done 2026-11-15: the elevated window acceptance passed (docs/milestone-9.md).
2. Download the draft's zip on this PC, compare the SHA-256, start `snisplit-gui.exe`
   from it and see whether Windows or the antivirus objects.
3. Publish the draft, then ask a friend to install from it with only the README.
