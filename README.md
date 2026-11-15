# snisplit

snisplit gets a short list of domains through an internet provider that drops
connections by looking at the server name in the TLS ClientHello (SNI) or the HTTP
`Host` header. It runs on your own PC, touches only the domains you list, works for
every app that uses the system's name resolution, and needs no kernel driver.

It is a small tool for the author and friends on similar providers, not a VPN or a
general proxy. It never decrypts traffic and holds no certificate.

Status: Windows, x86_64; tested on Windows 11 only. Linux and macOS are planned.

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
the rules are removed.

## Install

1. Download `snisplit-<version>-windows-x86_64.zip` from the Releases page and unpack
   it anywhere. There is no installer and nothing to uninstall: delete the folder to
   remove it.
2. Optional: compare the SHA-256 of the zip with the `.sha256` file next to it:
   `Get-FileHash snisplit-<version>-windows-x86_64.zip -Algorithm SHA256`.

The executables are not code-signed, so Windows SmartScreen may ask before the first
start, and an antivirus may warn about a program it has not seen before that changes
DNS rules. See "Antivirus" below before you decide to allow it.

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
- **Sites**: the ones that come with snisplit (Steam, Medium) and the ones you added,
  each with an On/Off switch. Only the sites switched on go through snisplit. Changing a
  switch while snisplit is on restarts it for about 2 seconds. Press a site's name to
  open it: its domains, Test (tests the site now, with snisplit on or off), Remove for
  sites you added, and "What snisplit tried".
- **Add a site**: type an address, or paste a link (it is shortened to the address), and
  press Test. If snisplit can open the site, press Add and turn on.
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
to that combination. On the author's PC, Kaspersky allows the current foreground
builds; it terminated an earlier experimental build that installed itself as a
Windows service, and that feature was removed.

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
alone, `cargo build --release -p snisplit`, needs 1.85. `cargo test --workspace` runs the test suite; no test needs
administrator rights or the internet. The plan, the measurements behind each design
choice and the per-milestone records are in `PLAN.md` and `docs/`.

## Licence

MIT or Apache-2.0, at your option.
