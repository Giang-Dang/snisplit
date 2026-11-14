param(
  [Parameter(Mandatory)] [string] $Upstream,
  [ValidateSet('single','split1')] [string] $Mode = 'split1',
  [string] $Listen = '127.0.0.2',
  [int] $Port = 443,
  [int] $Seconds = 40,
  [int] $GapMs = 30
)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;

public static class Relay {
    static byte[] ReadFirstRecord(NetworkStream s) {
        var buf = new MemoryStream();
        var chunk = new byte[16384];
        int need = 5;
        while (buf.Length < need) {
            int n = s.Read(chunk, 0, chunk.Length);
            if (n == 0) break;
            buf.Write(chunk, 0, n);
            var b = buf.GetBuffer();
            if (b[0] != 0x16) break;
            if (buf.Length >= 5) need = 5 + (b[3] << 8 | b[4]);
        }
        return buf.ToArray();
    }

    static long Pump(Stream from, Stream to, Socket toSocket) {
        long total = 0;
        var tmp = new byte[16384];
        try { int n; while ((n = from.Read(tmp, 0, tmp.Length)) > 0) { to.Write(tmp, 0, n); total += n; } }
        catch (Exception) { }
        try { toSocket.Shutdown(SocketShutdown.Send); } catch (Exception) { }
        return total;
    }

    static string Handle(TcpClient client, string upstream, int port, bool split, int gapMs) {
        using (client)
        using (var up = new TcpClient()) {
            try {
                up.NoDelay = true;
                client.NoDelay = true;
                var cs = client.GetStream();
                var first = ReadFirstRecord(cs);
                up.Connect(upstream, port);
                var us = up.GetStream();
                string pieces;
                if (split && first.Length > 1) {
                    us.Write(first, 0, 1); us.Flush();
                    Thread.Sleep(gapMs);
                    us.Write(first, 1, first.Length - 1); us.Flush();
                    pieces = "1+" + (first.Length - 1);
                } else {
                    us.Write(first, 0, first.Length); us.Flush();
                    pieces = first.Length.ToString();
                }
                var toServer = Task.Run(() => Pump(cs, us, up.Client));
                var fromServer = Task.Run(() => Pump(us, cs, client.Client));
                Task.WaitAll(new Task[] { toServer, fromServer }, 30000);
                string down = fromServer.IsCompleted ? fromServer.Result.ToString() : "open";
                return "first_record=" + first.Length + " pieces=" + pieces + " bytes_from_server=" + down;
            } catch (Exception e) {
                while (e.InnerException != null) e = e.InnerException;
                return "error " + e.GetType().Name + ": " + e.Message;
            }
        }
    }

    public static string[] Run(string listen, int port, string upstream, bool split, int gapMs, int seconds) {
        var listener = new TcpListener(IPAddress.Parse(listen), port);
        listener.Start();
        var results = new ConcurrentQueue<string>();
        var tasks = new List<Task>();
        var end = DateTime.UtcNow.AddSeconds(seconds);
        while (DateTime.UtcNow < end) {
            if (listener.Pending()) {
                var c = listener.AcceptTcpClient();
                tasks.Add(Task.Run(() => results.Enqueue(Handle(c, upstream, port, split, gapMs))));
            } else {
                Thread.Sleep(50);
            }
        }
        listener.Stop();
        Task.WaitAll(tasks.ToArray(), 10000);
        return results.ToArray();
    }
}
'@

"relay $Listen`:$Port -> $Upstream`:$Port mode=$Mode for ${Seconds}s at $(Get-Date -Format s)"
$ip = ([System.Net.Dns]::GetHostAddresses($Upstream) | Where-Object AddressFamily -eq 'InterNetwork' | Select-Object -First 1).ToString()
"upstream address $ip"
[Relay]::Run($Listen, $Port, $ip, $Mode -eq 'split1', $GapMs, $Seconds) | ForEach-Object { "conn: $_" }
