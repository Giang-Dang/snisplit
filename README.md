# snisplit

snisplit gets a short list of domains through an internet provider that drops
connections by looking at the server name in the TLS ClientHello (SNI) or the HTTP
`Host` header. It runs on your own PC, touches only the domains you list, works for
every app that uses the system's name resolution, and needs no kernel driver.

It is a small tool for the author and friends on similar providers, not a VPN or a
general proxy. It never decrypts traffic and holds no certificate.

Status: Windows, x86_64; tested on Windows 11 only. Linux x86_64: the command line
only, checked in containers (see "Linux" below), not yet on a real machine or a Steam
Deck. macOS is planned.

## How it works

```
app --DNS query--> snisplit DNS (loopback) --listed?--> answer: a loopback address
                                           \--other----> your normal DNS server
app --TCP 443/80--> snisplit relay (loopback)
                      finds the server name in the first bytes
                      connects to the real server
                      TLS: sends the ClientHello in two or three pieces
                      HTTP: writes the header name as "hoSt:" on every request
                      then copies bytes both ways unchanged
```

While it runs, Windows name resolution rules (NRPT) send only the listed domains to
snisplit. Every other name resolves and connects exactly as before. When it stops,
the rules are removed. On Linux, systemd-resolved does the same job: snisplit adds one
small network link per profile (`snisplit0`, `snisplit1`, ...) that carries the listed
domains, and deletes the links when it stops.

## Install

### Installer (recommended)

1. Download `snisplit-<version>-windows-x86_64.msi` and double-click it. Accept the
   licence and confirm the Windows prompt; snisplit installs for all users into
   `C:\Program Files\snisplit`, a folder only administrators can change, which matters
   for a program that runs elevated.
2. Start it from the Start menu (snisplit). The command line `snisplit` is on the
   system PATH; open a new elevated terminal to use it.
3. To remove it, use Settings, Apps, Installed apps, snisplit, Uninstall. The
   uninstaller first removes any DNS rules snisplit left behind (as `snisplit cleanup`
   does), then the program files, the Start menu entry, the PATH entry and the lock
   folder `%ProgramData%\snisplit`. Your sites in `%APPDATA%\snisplit` are kept.

A newer installer upgrades an older installation in place; an older one refuses to
install over a newer one. Silent install for scripts:
`msiexec /i snisplit-<version>-windows-x86_64.msi /qn`.

### Zip

1. Download `snisplit-<version>-windows-x86_64.zip` from the Releases page and unpack
   it anywhere. Delete the folder to remove it.

Optional for either: compare the SHA-256 of the download with the `.sha256` file next
to it: `Get-FileHash <file> -Algorithm SHA256`.

The executables and the installer are not code-signed, so Windows SmartScreen may ask
before the first start and the installer shows "Unknown publisher", and an antivirus
may warn about a program it has not seen before that changes DNS rules. See
"Antivirus" below before you decide to allow it.

## Use

snisplit needs administrator rights while it runs, because adding and removing the
DNS rules needs them.

### Window

Right-click `snisplit-gui.exe` and choose Run as administrator, or just start it and
confirm the Windows prompt. The window is small (400 px wide) and has:

- **One round button.** Its word says what pressing it does (Start, Cancel, Stop, Try
  again); the line under it says what snisplit is doing (Off, Starting..., On, Applying
  your change..., Turning off...). A hollow ring is off, a turning arc is busy, a filled
  button is on, a double ring means something went wrong. Stopping, or closing the
  window, removes the DNS rules before the program exits; keep the window open while
  you use the sites.
- **Sites**: the ones that come with snisplit (Steam, Medium) and the ones you added
  are behind one "Sites" row that shows which are on. Press it to show or hide the list;
  the list starts hidden each time the window opens. While it is hidden the window fits
  its content; while the list or Activity is shown you can resize the window. Each site
  has an On/Off switch. Only the sites switched on go through snisplit. Changing a
  switch while snisplit is on restarts it for about 2 seconds. Press a site's name to
  open it: its domains, Test (tests the site now, with snisplit on or off), Remove for
  sites you added, and "What snisplit tried".
- **Add a site**: at the bottom of the Sites list. Type an address, or paste a link (it
  is shortened to the address), and press Test. If snisplit can open the site, press Add
  and turn on.
- **Activity**: what snisplit did, in more technical words. "Save to a file" writes
  `snisplit-log.txt` to your Desktop, to send to whoever helps you.

If snisplit cannot start, the window says why in plain words (another program is in the
way, or Windows refused the change because it was not started as administrator); "See
what happened" shows the details. If a stop could not remove the DNS rules, the window
says "Off, with leftovers" and offers "Remove leftover settings"; pressing Start also
removes them.

Sites you add are stored as small text files in `%APPDATA%\snisplit\sites`; the sites
switched off are listed by name in `%APPDATA%\snisplit\off.list`. The command line reads
the same folder and the same list.

### Command line

Open PowerShell or Terminal as administrator in the unpacked folder:

```
.\snisplit run
```

It prints one line when it is ready (about 2 seconds) and relays the built-in
profiles, Steam and medium.com, until you press Ctrl+C or close the window. Start it
when you need it; leave the window open while you use the sites.

Other commands:

| Command | What it does |
|---|---|
| `snisplit probe <domain>` | Tests whether a domain is blocked on your connection and which method gets it through, and prints a profile for it. Needs no administrator rights. |
| `snisplit show-profile <name>` | Prints a built-in profile (`steam`, `medium`) as a template. |
| `snisplit cleanup` | Removes snisplit's DNS rules. Only needed if the process was killed; the next `run` also removes leftovers. |
| `snisplit help` | All options. |

## Adding a site from the command line

1. Ask whether snisplit can help:

   ```
   .\snisplit probe example.com
   ```

   The table shows what happens to a plain connection and to each method. The verdict
   is one of: not blocked here (nothing to add), a suggested profile, or no method
   worked.

2. Save the suggested profile as plain ASCII text in `%APPDATA%\snisplit\sites`, in
   a file called `<name>.profile` where `<name>` is the profile's `name` line. In
   Windows PowerShell 5.1 do not use `>`, which writes UTF-16; paste the text into
   Notepad and save it, or pipe it through `Set-Content -Encoding ascii`.

3. `.\snisplit run` relays the built-in profiles plus every file in that folder,
   except the sites named in `%APPDATA%\snisplit\off.list` (one name per line, as the
   window's switches write it; a name that matches no site is ignored). A file with a
   mistake is reported by name and skipped. With every site switched off, `run` stops
   with an error before it changes anything. To run a chosen set instead, name them:
   `--profile steam --profile-file my-sites.profile`; `off.list` is not used then.

A profile file looks like this:

```
name my-sites
domains example.com
domains example.net
tls-ports 443
tls-split 1
http-ports 80
http-method host-spell
```

A listed domain covers its subdomains. `tls-split` lists where the ClientHello is cut:
a byte offset, `host-start`, or `domain-mid`.

## Linux

The command line works on Linux x86_64 as one static file; there is no window on Linux
yet. snisplit needs systemd-resolved as the system resolver, and root while it runs.

### Which systems

Checked in containers (DNS routing, the relay, Ctrl+C, a killed process and `cleanup`):

| System | What to do first |
|---|---|
| Ubuntu 24.04, Fedora, SteamOS 3.8 | Nothing: systemd-resolved is already the resolver |
| Ubuntu 22.04 | Nothing on a desktop install (systemd-resolved is the default there) |
| Debian, Arch, openSUSE before the steps below | snisplit refuses to start and changes nothing |
| Debian 12 and 13 | `sudo apt install systemd-resolved` (checked without NetworkManager; on a desktop, restart once afterwards so NetworkManager hands its DNS servers to systemd-resolved) |
| Arch Linux | `sudo systemctl enable --now systemd-resolved` and `sudo ln -sf ../run/systemd/resolve/stub-resolv.conf /etc/resolv.conf` |
| openSUSE Tumbleweed | `sudo zypper install systemd-resolved`, `sudo systemctl enable --now systemd-resolved` and `sudo ln -sf ../run/systemd/resolve/stub-resolv.conf /etc/resolv.conf` (checked without NetworkManager or netconfig; on a desktop, NetworkManager may also need `dns=systemd-resolved`) |

Linux Mint, Pop!_OS, Manjaro, EndeavourOS and Bazzite were not checked. If
`snisplit run` says "needs systemd-resolved as the system resolver", enable it the way
your distribution documents, so that `/etc/resolv.conf` lists `nameserver 127.0.0.53`.

### Install and run

1. Download `snisplit-<version>-linux-x86_64.tar.gz`, unpack it anywhere in your home
   folder, and open a terminal in that folder.
2. Run `sudo ./snisplit run`. It prints one line when it is ready and relays Steam and
   medium.com until you press Ctrl+C or close the terminal. Leave it open while you use
   the sites.
3. Sites you add go in `~/.config/snisplit/sites` as `<name>.profile` files (see "Adding a
   site from the command line"). Under `sudo` snisplit reads your folder, not root's.

`snisplit probe <domain>` needs no root. If snisplit was killed, `sudo ./snisplit
cleanup` removes its links; the next `run` removes them too.

### Steam Deck (SteamOS)

1. Switch to Desktop Mode and open Konsole.
2. The `deck` user has no password at first, and `sudo` needs one: run `passwd` once.
3. Unpack snisplit under `/home/deck`; nothing is installed into the read-only system.
4. Run `sudo ./snisplit run` and keep Konsole open.

Not checked yet: whether snisplit keeps running after you switch back to Gaming Mode,
and the Steam client itself. Tell the author what you see.

## What it cannot do

- Sites blocked by IP address, or whose real address is hidden by DNS tampering.
  `probe` reports "no method worked" for these.
- Apps that ignore the system's name resolution: a browser with "secure DNS" set to a
  specific provider, or an app that connects to fixed addresses. Turn the browser's
  secure DNS off or set it to use the system resolver.
- While snisplit's rules exist, Chromium-based browsers stop upgrading to secure DNS
  on their own for all names. That is how Windows and Chromium treat any NRPT rule.
- Another provider may filter differently. The methods here were measured on one
  provider; `probe` tells you what works on yours.

## Antivirus

snisplit starts a hidden PowerShell process to add and remove DNS rules, listens on
loopback ports 53, 80 and 443, and is unsigned and new. Security software can react
to that combination. On the author's PC, Kaspersky ran the current foreground builds
without an alert in the acceptance runs. Once, a file scan flagged one local build of
the window with a generic cloud verdict and removed the file; rebuilt files from the
same source were not flagged, so the cause is unknown. The window binary in the 0.2.0 archive built by the release
workflow was also flagged by the same product with a generic verdict, while the
command-line binary and local builds of the same source were not. The cause is
unknown here too, and for this reason 0.2.0 has not been published as a release;
building from source remains the documented route. An earlier experimental build
that installed itself as a Windows service was terminated, and that feature was removed.

If your antivirus blocks snisplit, the decision to allow it is yours. Read the source
or build it yourself first if you are unsure:

```
cargo build --release
```

## If something goes wrong

- Listed sites stop opening after a restart, or after snisplit was killed: open
  snisplit once and press "Remove leftover settings" (or Start), or run
  `snisplit cleanup` as administrator.
- "binding 127.0.0.2:53 failed" or similar: another program holds that loopback
  address and port. Stop it, or stop the other copy of snisplit.
- A site still fails: run `snisplit probe <domain>` and read the verdict; restart the
  browser so it drops connections and DNS answers it cached before snisplit started.

## Building and tests

Rust 1.95 or newer for the whole workspace (the window needs it); the command line
alone, `cargo build --release -p snisplit`, needs 1.89. The Linux release file is built
with `cargo build --release -p snisplit --target x86_64-unknown-linux-musl`;
`tools/acceptance/linux/matrix.sh` runs the container checks (`docs/milestone-10.md`). `cargo test --workspace` runs the test suite; no test needs
administrator rights or the internet. The plan, the measurements behind each design
choice and the per-milestone records are in `PLAN.md` and `docs/`.

The installer needs the .NET SDK (8 or newer) and is built with the WiX Toolset 5,
which `dotnet` restores on its own: after `cargo build --release`, run
`dotnet build installer -c Release -p:ProductVersion=<workspace version>`; the package
lands in `target\installer\bin\Release`. `docs/installer.md` has the design.

## Licence

MIT or Apache-2.0, at your option.
