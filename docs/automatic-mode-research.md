# Automatic mode: local proxy research

Research only, 2026-11-15. Marks: [read] = read in a doc or source cited; [inf] = inference;
[summary] = only a web search summary, page not read.

## 1. Answer

Depends, leaning yes for browsers only. A proxy auto-config (PAC) script returning
`PROXY 127.0.0.1:N; DIRECT`, served over loopback http by the tool itself, fails open in
Chromium by its documentation and in Firefox by a fixed bug report; WinINET/WinHTTP
clients are undetermined. The relay already holds the parts that matter (buffered first
flight, cut writer, reply classifier). Against it: a fixed proxy setting fails closed;
Steam, command-line tools, games and Store apps are outside or unproven; `decide` refuses
unlisted hosts; and evidence that byte-1 splitting is harmless for unfiltered sites is two
cases. It also reverses PLAN.md "Not in scope: ... an all-traffic mode, automatic domain
discovery, general proxy/VPN features" and README.md:8-9, so it is an owner decision first.

## 2. Findings

**Q1 Fail-open.**
- Chromium tries PAC entries in order; a failed proxy is marked "bad for the next 5 minutes";
  only "connection level failures" trigger fallback [read,
  https://chromium.googlesource.com/chromium/src/+/HEAD/net/docs/proxy.md].
- One fixed proxy that is down: requests "fail with ERR_PROXY_CONNECTION_FAILED" (same page)
  [read]; PowerTunnel issue 152 shows that after a stop [read,
  https://github.com/krlvm/PowerTunnel/issues/152]. So: no fixed proxy.
- PAC fetch failure under Windows system settings: "the specified manual proxy servers will
  be used", otherwise "silently fall back to DIRECT"; a fetched script stays fresh up to 12 h;
  failed fetches back off to every 4 h (same page) [read]. A dead tool means DIRECT for a new
  browser; a running browser keeps the script and falls back per connection [inf].
- Firefox: bug 1779005, "no instant fallback to direct connection when proxy became
  unreachable", fixed in 105 and 102.3esr [read, https://bugzilla.mozilla.org/show_bug.cgi?id=1779005].
- Local file: WinHTTP "will not use proxy script files from file:// URLs, on any version of
  Windows"; IE11 disabled WinINET file:// scripts [read,
  https://learn.microsoft.com/en-us/archive/blogs/ieinternals/understanding-web-proxy-configuration].
- API: `InternetSetOption` with `INTERNET_OPTION_PER_CONNECTION_OPTION`,
  `PROXY_TYPE_AUTO_PROXY_URL`, `INTERNET_PER_CONN_AUTOCONFIG_URL`, then
  `INTERNET_OPTION_SETTINGS_CHANGED` and `INTERNET_OPTION_REFRESH` [read,
  https://learn.microsoft.com/en-us/windows/win32/wininet/option-flags,
  https://learn.microsoft.com/en-us/windows/win32/api/wininet/ns-wininet-internet_per_conn_optiona]. All exist in
  windows-sys 0.61.2 behind feature `Win32_Networking_WinInet`
  (`src/Windows/Win32/Networking/WinInet/mod.rs`), not enabled in Cargo.toml:17 [read].
- Rights: "individual (even non-admin) users can set their own proxy settings" unless the
  `ProxySettingsPerUser` policy is 0 (ieinternals page) [read].
- Leftovers on kill: the setting stays [inf]. GreenTunnel issue 168: forced close at
  shutdown, then "No Internet connection" [read,
  https://github.com/SadeghHayeri/GreenTunnel/issues/168].

**Q2 Who honours it.**
- Chromium: the proxy page discusses "using system proxy settings" on Windows [read]; that
  it is the default [inf].
- Firefox follows WinINET "only when configured to Use System Proxy Settings"; WinHTTP apps
  "may or may not" (ieinternals page) [read]. Firefox default = system since 3.6.4 [summary].
- Electron: with no proxy configuration "Electron falls back to the system settings" unless
  command-line options intervene [read, https://www.electronjs.org/docs/latest/api/structures/proxy-config].
- curl reads proxy environment variables; "curl does not support PAC files" [read,
  https://everything.curl.dev/usingcurl/proxies/pac.html].
- Store apps: an IoT Core page requires `CheckNetIsolation.exe LoopbackExempt -a` per
  package for loopback [read,
  https://learn.microsoft.com/en-us/previous-versions/windows/iot-core/develop-your-app/loopback];
  that desktop Store apps need the same [inf].
- Steam client and its browser [summary]; games: could not determine.

**Q3 Elevation.**
- Same account: elevated apps share "the user's registry profile, HKEY_CURRENT_USER" [read,
  https://learn.microsoft.com/en-us/previous-versions/technet-magazine/dd822916(v=msdn.10)].
- Over-the-shoulder with another admin account: it would change that admin's settings [inf].
- Only `snisplit-gui` carries `requireAdministrator` (crates/gui/build.rs:5); `snisplit
  probe` already runs unelevated (README.md:94) [read]. A proxy mode needs no elevation
  [inf], so the GUI would need a second executable or an unelevated launcher [inf].

**Q4 Protocol.**
- HTTPS: CONNECT; authority-form is "only used for CONNECT requests"; after 2xx both sides
  "switch to tunnel mode" [read, RFC 9112 3.2.3, RFC 9110 9.3.6]. Plain HTTP: "a client MUST
  send the target URI in absolute-form" [read, RFC 9112 3.2.2].
- SOCKS: not needed [inf]; Windows settings have HTTP and Secure fields, SOCKSv4 resolves
  names client side (Chromium page), IE "only supports SOCKSv4" (ieinternals) [read].
- DNS: for HTTP proxies "name resolution is always deferred to the proxy" [read], so browser
  secure DNS stops mattering for proxied requests [inf]. ECH under a proxy: could not
  determine.
- QUIC: a 2014 Chromium review "Do not use QUIC for requests that are through a proxy"
  [read, https://codereview.chromium.org/217783003]; current code not checked; milestone 8 found QUIC unfiltered here, so a leak would not hurt [read].
- Reusable: `collect_first_flight` (connection.rs:315), `open_upstream` (connection.rs:350),
  `write_pieces`, `HostRespeller`, `reply_outcome` (probe/reply.rs:85). Missing: CONNECT
  parsing and the 200 reply (`HostRespeller` stops on CONNECT, http_stream.rs:8;
  `inspect_request` reads only `Host:`, http.rs:16-33); a catch-all, since `decide` refuses
  `NotListed` (first_flight.rs:81-83) and `DomainMid` needs a matched domain; absolute-form
  handling; per-request upstreams, since one proxy connection may carry several origins
  [inf]; a loopback http server for the PAC; IPv6 (`build_a_query` asks A only, dns.rs:68).

**Q5 One method for all.**
- Harmless? Two unfiltered cases answered every cut: one site, 2 runs
  (milestone-6.md:228-245), and one name on an unfiltered address, 3 runs (:261-277) [read].
  Both used the probe's synthetic hello, not a browser's (milestone-6.md:96-108) [read]. Not
  established.
- ByeDPI `--auto` groups methods with triggers `torst` (timeout or reset after the first
  request), `redirect` (Location to another domain), `ssl_err` (no ServerHello), caching
  per IP, default 28 h [read, https://raw.githubusercontent.com/hufrea/byedpi/main/README.md].
  Per-IP matters: one name was filtered on one address and not another (milestone-6.md:315-316)
  [read]. ByeByeDPI's checker counts a body shorter than `Content-Length` as "Block detected"
  (SiteCheckUtils.kt:109-114) and its README says settings may need adjusting (README-en.md:21)
  [read].
- `Outcome` (Reset, TimedOut, alert, handshake) gives the signals; `assess` (assess.rs:102)
  judges a finished table of synthetic attempts, not a live connection, so
  retry-and-remember is new code [read/inf].
- Retry is transparent only while no server byte has reached the client and the relay holds
  the first flight [inf]. Exceptions: TLS 1.3 early data, "no guarantees of non-replay between
  connections" [read, RFC 8446 2.3]; a plain-HTTP POST may already have reached the server [inf].

**Q6 Antivirus.** Microsoft lists "Redirect web traffic without giving notice and getting
consent" as unwanted software [read, https://learn.microsoft.com/en-us/unified-secops/criteria].
Kaspersky categories [summary]. PLAN section 6 records a terminated install and deleted
files on this PC [read]. Only a test can settle it.

**Q7 Prior art.** GreenTunnel: `--system-proxy` default true, DoH default [read, README];
issue 168 [read]. PowerTunnel: `--auto-proxy-setup-ie` [read, README]; issue 152 [read].
Neither README documents failure modes or leftover settings [read]. ByeDPI: a local SOCKS
proxy with a Windows build target; `--fake` "may work unstably on Windows" [read]. SpoofDPI on Windows:
could not determine.

## 3. What a prototype must test with the owner present

- [ ] Chrome, Edge, Firefox with PAC `PROXY ...; DIRECT`: time to first page with the tool
      killed, browser opened before and after the kill.
- [ ] A WinINET and a WinHTTP client with the tool dead.
- [ ] Tool restarted while a browser runs: does the mode come back.
- [ ] Running browsers pick up the change, from an unelevated and an elevated writer.
- [ ] Kill, reboot, log off: the setting afterwards and what the user sees.
- [ ] Steam client, its browser, a game download: which traffic reaches the proxy.
- [ ] Byte-1 split on a sample of unfiltered sites, ports 443 and 80.
- [ ] Antivirus: alert name and category; process terminated or file removed; setting
      blocked or reverted, and at which step; elevated versus unelevated.

## 4. Decisions the owner would have to make

- Lift the all-traffic, automatic-discovery and general-proxy exclusions (PLAN 2, README).
- Second unelevated executable, or one executable with two launch paths.
- Enable windows-sys `Win32_Networking_WinInet`; CONNECT parsing hand-written or a crate.
- Programs in scope (browsers only, or Steam too); IPv6.

## 5. Could not determine

Fallback speed in numbers; WinINET/WinHTTP on a dead PAC URL; file:// PAC in Chrome, Edge
and Firefox; fixed-proxy behaviour outside Chromium; Firefox default from source; current
Chromium QUIC and ECH behaviour behind a proxy; Steam, Electron apps and games in practice;
SpoofDPI on Windows; registry value names from a primary source.
