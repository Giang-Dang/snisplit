param(
  [Parameter(Mandatory)] [string] $HostName,
  [ValidateSet('tls','http')] [string] $Proto = 'tls',
  [string[]] $Modes = @('single','split1','splithost','split1+host'),
  [int] $Rounds = 3,
  [int] $GapMs = 30,
  [string] $Ip
)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Net.Sockets;
using System.Threading;
using System.Collections.Generic;

public class SplitStream : Stream {
    readonly NetworkStream inner;
    readonly Func<byte[], int, int, int[]> cutter;
    readonly int gapMs;
    bool firstDone;
    public string Pieces = "";
    public SplitStream(NetworkStream inner, Func<byte[], int, int, int[]> cutter, int gapMs) {
        this.inner = inner; this.cutter = cutter; this.gapMs = gapMs;
    }
    public override void Write(byte[] buffer, int offset, int count) {
        if (firstDone) { inner.Write(buffer, offset, count); return; }
        firstDone = true;
        var cuts = new List<int>(cutter(buffer, offset, count));
        cuts.Add(count);
        int start = 0;
        var sizes = new List<string>();
        foreach (var cut in cuts) {
            if (cut <= start || cut > count) continue;
            inner.Write(buffer, offset + start, cut - start);
            inner.Flush();
            sizes.Add((cut - start).ToString());
            start = cut;
            if (start < count && gapMs > 0) Thread.Sleep(gapMs);
        }
        Pieces = string.Join("+", sizes);
    }
    public override int Read(byte[] buffer, int offset, int count) { return inner.Read(buffer, offset, count); }
    public override void Flush() { inner.Flush(); }
    public override bool CanRead { get { return true; } }
    public override bool CanWrite { get { return true; } }
    public override bool CanSeek { get { return false; } }
    public override bool CanTimeout { get { return true; } }
    public override int ReadTimeout { get { return inner.ReadTimeout; } set { inner.ReadTimeout = value; } }
    public override int WriteTimeout { get { return inner.WriteTimeout; } set { inner.WriteTimeout = value; } }
    public override long Length { get { throw new NotSupportedException(); } }
    public override long Position { get { throw new NotSupportedException(); } set { throw new NotSupportedException(); } }
    public override long Seek(long o, SeekOrigin s) { throw new NotSupportedException(); }
    public override void SetLength(long v) { throw new NotSupportedException(); }
}

public static class Cuts {
    public static int IndexOf(byte[] buf, int offset, int count, byte[] needle) {
        for (int i = 0; i + needle.Length <= count; i++) {
            bool ok = true;
            for (int j = 0; j < needle.Length; j++) {
                byte a = buf[offset + i + j], b = needle[j];
                if (a >= 65 && a <= 90) a += 32;
                if (a != b) { ok = false; break; }
            }
            if (ok) return i;
        }
        return -1;
    }
    public static Func<byte[], int, int, int[]> For(string mode, string host) {
        byte[] h = System.Text.Encoding.ASCII.GetBytes(host.ToLowerInvariant());
        return (buf, offset, count) => {
            int at = IndexOf(buf, offset, count, h);
            int mid = at < 0 ? -1 : at + h.Length / 2;
            switch (mode) {
                case "single": return new int[0];
                case "split1": return new[] { 1 };
                case "splithost": return mid < 0 ? new int[0] : new[] { mid };
                case "split1+host": return mid < 0 ? new[] { 1 } : new[] { 1, mid };
                default: throw new ArgumentException(mode);
            }
        };
    }
}
'@

function Connect-Target {
  $tcp = [System.Net.Sockets.TcpClient]::new()
  $tcp.NoDelay = $true
  $tcp.ReceiveTimeout = 8000
  $tcp.SendTimeout = 8000
  $target = if ($Ip) { $Ip } else { $HostName }
  $port = if ($Proto -eq 'tls') { 443 } else { 80 }
  $tcp.Connect($target, $port)
  $tcp
}

function Probe-Tls($mode) {
  $tcp = Connect-Target
  try {
    $split = [SplitStream]::new($tcp.GetStream(), [Cuts]::For($mode, $HostName), $GapMs)
    $ssl = [System.Net.Security.SslStream]::new($split, $false)
    $ssl.ReadTimeout = 8000
    $ssl.AuthenticateAsClient($HostName)
    $req = [Text.Encoding]::ASCII.GetBytes("HEAD / HTTP/1.1`r`nHost: $HostName`r`nConnection: close`r`n`r`n")
    $ssl.Write($req); $ssl.Flush()
    $line = [IO.StreamReader]::new($ssl).ReadLine()
    "PASS pieces=$($split.Pieces) tls=$($ssl.SslProtocol) status='$line'"
  } catch {
    $e = $_.Exception; while ($e.InnerException) { $e = $e.InnerException }
    "FAIL pieces=$($split.Pieces) $($e.GetType().Name): $($e.Message)"
  } finally { $tcp.Close() }
}

function Probe-Http($mode) {
  $tcp = Connect-Target
  try {
    $split = [SplitStream]::new($tcp.GetStream(), [Cuts]::For($mode, $HostName), $GapMs)
    $req = [Text.Encoding]::ASCII.GetBytes("HEAD / HTTP/1.1`r`nHost: $HostName`r`nConnection: close`r`n`r`n")
    $split.Write($req, 0, $req.Length); $split.Flush()
    $line = [IO.StreamReader]::new($split).ReadLine()
    if ($line -match '^HTTP/1\.[01] \d{3}') { "PASS pieces=$($split.Pieces) status='$line'" } else { "FAIL pieces=$($split.Pieces) got='$line'" }
  } catch {
    $e = $_.Exception; while ($e.InnerException) { $e = $e.InnerException }
    "FAIL pieces=$($split.Pieces) $($e.GetType().Name): $($e.Message)"
  } finally { $tcp.Close() }
}

"probe host=$HostName proto=$Proto ip=$Ip gap=${GapMs}ms rounds=$Rounds at $(Get-Date -Format s)"
foreach ($round in 1..$Rounds) {
  foreach ($mode in $Modes) {
    $r = if ($Proto -eq 'tls') { Probe-Tls $mode } else { Probe-Http $mode }
    "r{0} {1,-12} {2}" -f $round, $mode, $r
  }
}
