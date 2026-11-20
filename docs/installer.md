# Windows installer

Built 2026-11-20 at the owner's request ("an installer for Windows 11 following modern
best practices"). It reverses milestone-7 decision 2 ("one zip, no installer") and the
"no installer" reason in PLAN.md D15: the zip stays, and an MSI is added next to it.

## Decisions

1. **MSI, built with WiX Toolset 5.0.2** (`installer/`, `WixToolset.Sdk` through
   `dotnet build`). MSIX was not chosen: an unsigned MSIX does not install without
   developer mode or a trusted certificate, and the window needs
   `requireAdministrator`, which MSIX allows only through a restricted capability.
   Inno Setup and NSIS make an EXE that winget, Intune and Group Policy handle less
   well than an MSI. WiX 5 is MS-RL with no extra terms. WiX 6 introduced the Open
   Source Maintenance Fee and WiX 7 (April 2026) blocks every build until the EULA is
   accepted (`AcceptEula=wix7`); accepting it is a licence commitment for everyone who
   builds, so it is left to the owner. Moving to 7 is a version bump plus that property.
2. **Per machine, into `C:\Program Files\snisplit`.** snisplit runs elevated; an
   elevated program in a folder its user can write to is a privilege-escalation path.
   Program Files is writable by administrators only.
3. **What is installed**: `snisplit-gui.exe`, `snisplit.exe`, `README.md`,
   `LICENSE-MIT`, `LICENSE-APACHE`; an all-users Start menu shortcut to the window; the
   install folder appended to the system PATH; an Apps entry with links to the
   repository and to "If something goes wrong", with Modify and Repair hidden (there is
   one feature and nothing to repair that a reinstall does not).
4. **Upgrades**: one fixed `UpgradeCode` (`6b633373-174d-472c-b1a8-0998e2529b9e`) and
   `MajorUpgrade` with `AllowSameVersionUpgrades`, so a newer MSI, or a rebuild of the
   same version (a local 0.2.0 over a CI 0.2.0), replaces the installed one instead of
   adding a second Apps entry, and an older MSI refuses. ICE61, which warns about the
   same-version case, is suppressed for that reason. The
   version comes from the workspace version through `-p:ProductVersion`, which the
   release workflow reads with `cargo metadata` as it does for the zip; the build fails
   without it.
5. **Uninstall removes leftover DNS rules.** A deferred custom action runs
   `"[INSTALLFOLDER]snisplit.exe" cleanup` as LocalSystem (`Impersonate="no"`), hidden
   through WiX's `WixQuietExec`, before `RemoveFiles`, only on a full uninstall
   (`REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE`), and its failure does not stop the
   uninstall (`Return="ignore"`). `cleanup` needs only `ProgramData`, which LocalSystem
   has, and removes the rules even while another snisplit holds the lock. An upgrade
   does not run it: the files are replaced and the next start removes leftovers itself.
6. **Kept and removed data**: `%ProgramData%\snisplit\instance.lock` and its folder
   are removed on uninstall; `%APPDATA%\snisplit` (the user's sites and `off.list`) is
   kept, as Windows apps do with user data.
7. **ICE43 and ICE57 are suppressed.** Both flag the Start menu shortcut because ICE
   validation treats `ProgramMenuFolder` as per-user; this package sets `ALLUSERS=1`
   (checked in the built MSI's Property table), so the folder is the all-users Start
   menu and the file key path is correct. Moving the shortcut to its own component
   with an HKMU key path was built and only traded these for ICE57 and ICE69; an HKCU
   key path was not tried, since it would write to the installing administrator's hive
   in a per-machine package.
8. **UI**: `WixUI_Minimal` with `installer/license.rtf` (the dual licence in one
   paragraph, what snisplit changes, and that uninstall keeps the user's sites).
9. **Not done, for the owner**: code signing (an unsigned MSI shows "Unknown
   publisher" and SmartScreen asks), a winget manifest, and publishing.
10. **Icon** (2026-11-20, the owner chose concept D, "Sliced S", from five): a violet
   rounded tile with a white S cut across the middle, its halves shifted apart.
   `assets/icon/snisplit.svg` draws 40 to 256 px; `snisplit-small.svg` draws 16 to
   32 px with a thicker stroke and a wider, more offset cut so the cut still shows at
   16 px. `tools/icon/build_ico.py <rsvg-convert>` renders both into
   `assets/icon/snisplit.ico` (10 PNG images) and the 32 px window icon
   `snisplit-32.rgba`; the generated files are committed, so a build needs neither
   tool. The owner chose no new crate for embedding: `tools/icon/icon_resource.rs`
   writes a `.res` file (RT_ICON per image, one RT_GROUP_ICON) from the `.ico`, and
   the `build.rs` of `crates/gui` and `crates/cli` include it by `#[path]` and pass the
   `.res` to the MSVC linker, so both executables carry the icon. The window sets the
   same icon at run time (`with_icon`), replacing eframe's default. The MSI shows it
   for the Apps entry (`ARPPRODUCTICON`); the Start menu shortcut takes it from
   `snisplit-gui.exe`.

## Release workflow

`release.yml` builds the MSI after the zip from the same `target\release` binaries,
writes `<name>.msi.sha256`, uploads both with the zip as the workflow artifact and
attaches both to the draft release. The `windows-latest` runner has the .NET SDK.

## Checked

- `dotnet build installer -c Release -p:ProductVersion=0.2.0` on this PC (.NET SDK
  11 preview) restores WiX 5.0.2 and builds
  `target\installer\bin\Release\snisplit-0.2.0-windows-x86_64.msi` (3.8 MB) with ICE
  validation and no warnings.
- The built MSI, read back: `ALLUSERS=1`, summary platform `x64;1033`, version 0.2.0,
  the five files under `ProgramFiles64Folder\snisplit`, the PATH entry `set`, `last`,
  system, the custom action deferred, not impersonated, `Return="ignore"`, scheduled
  before `RemoveFiles` with the full-uninstall condition, and its entry point
  `WixQuietExec` present in the bundled `Wix4UtilCA_X64`.

## Icon checked

- `crates/cli/tests/icon_resource.rs`: a one-image `.ico` becomes the exact bytes the
  `.res` layout gives by hand (empty entry, icon entry padded to 4 bytes, group); a
  cursor file and an image past the end of the file are refused.
- A debug build: every image of the `.ico` is found byte for byte inside both
  executables, and `System.Drawing.Icon.ExtractIcon` returns the icon at 256, 32 and
  16 px from both, the 16 and 32 px ones being the small drawing.
- Not seen yet: the icon on a running window's title bar and taskbar button (the
  window needs elevation), and the release build, which could not be relinked while
  the owner's `snisplit-gui.exe` from `target\release` was open.

## Not yet run

Installing needs administrator rights, so nothing was installed. The owner's
elevated `tools/acceptance/m7b-installer.ps1 [-Version 0.2.0]` (unattended) installs
the MSI silently, checks the files (byte for byte against `target\release`), the PATH,
the shortcut and one Apps entry for the version, starts the installed `snisplit run`,
kills it and checks its rules are still there 10 s later, uninstalls silently, logs the
custom action's lines from the verbose uninstall log, and checks 0 rules, no install folder, no lock folder, no
shortcut, no PATH entry, no Apps entry and an unchanged `%APPDATA%\snisplit`. Still
open after that: whether Kaspersky reacts to `msiexec` running snisplit as LocalSystem
(the Windows service build of milestone 5 was terminated), an upgrade from one MSI
version to the next, and an uninstall with the window open (Windows Installer then
asks to close it).
