// Concurrency stress test for an EPBotNET.dll, mirroring epbot-core's run_auction().
// Bids every deal once on one thread (the reference), then `rounds` more times in shuffled order on
// `threads` threads, and reports auctions that differ from the reference.
//
// usage: [POOL=1] dotnet run stress.cs -- <EPBotNET.dll> <ns.bbsa> <ew.bbsa> <threads> <rounds> <pbn>...
//
// POOL=1 creates every EPBot instance before bidding starts and reuses them, so no constructor runs
// while other threads are bidding. Comparing POOL=0 and POOL=1 separates races during bidding from
// races during construction.
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Reflection;
using System.Text.RegularExpressions;

var asm = Assembly.LoadFrom(Path.GetFullPath(args[0]));
var botType = asm.GetTypes().First(t => t.Name == "EPBot" && t.IsClass && t.IsPublic);
MethodInfo M(string name, int n) => botType.GetMethods().First(m => m.Name == name && m.GetParameters().Length == n);
var newHand = M("new_hand", 6); var getBid = M("get_bid", 0); var setBid = M("set_bid", 3);
var setScoring = botType.GetProperty("scoring")?.SetMethod ?? M("set_scoring", 1);
var setConv = M("set_conventions", 3); var setSys = M("set_system_type", 2); var setOpp = M("set_opponent_type", 2);
var version = M("version", 0);
Console.WriteLine($"Engine version {version.Invoke(Activator.CreateInstance(botType), null)}  ({args[0]})");

var cards = new[] { File.ReadAllLines(args[1]), File.ReadAllLines(args[2]) };
int threads = int.Parse(args[3]), rounds = int.Parse(args[4]);

var deals = new List<(string pbn, int dealer, int vul)>();
foreach (var f in args.Skip(5))
{
    string dealer = "N", vul = "None";
    foreach (var line in File.ReadLines(f))
    {
        var m = Regex.Match(line, "^\\[(\\w+) \"(.*)\"\\]");
        if (!m.Success) continue;
        if (m.Groups[1].Value == "Dealer") dealer = m.Groups[2].Value;
        else if (m.Groups[1].Value == "Vulnerable") vul = m.Groups[2].Value;
        else if (m.Groups[1].Value == "Deal")
            deals.Add((m.Groups[2].Value, "NESW".IndexOf(dealer[0]),
                vul switch { "EW" => 1, "NS" => 2, "Both" or "All" => 3, _ => 0 }));
    }
}
Console.WriteLine($"{deals.Count} deals");

void ApplyCard(object bot, string[] lines, int side)
{
    foreach (var raw in lines)
    {
        var t = raw.Trim();
        if (t.Length == 0 || t[0] == '#' || t[0] == ';') continue;
        var p = t.Split('=', 2); if (p.Length != 2) continue;
        var key = p[0].Trim(); var val = p[1].Trim();
        if (int.TryParse(val, out var iv))
        {
            if (key.Equals("System type", StringComparison.OrdinalIgnoreCase)) setSys.Invoke(bot, [side, iv]);
            else if (key.Equals("Opponent type", StringComparison.OrdinalIgnoreCase)) setOpp.Invoke(bot, [side, iv]);
            else setConv.Invoke(bot, [side, key, iv == 1]);
        }
        else if (val.Equals("true", StringComparison.OrdinalIgnoreCase)) setConv.Invoke(bot, [side, key, true]);
        else if (val.Equals("false", StringComparison.OrdinalIgnoreCase)) setConv.Invoke(bot, [side, key, false]);
    }
}

bool pool = Environment.GetEnvironmentVariable("POOL") == "1";
object[] NewSet() => Enumerable.Range(0, 4).Select(_ => Activator.CreateInstance(botType)!).ToArray();
var sets = new ConcurrentBag<object[]>();
string Bid(string pbn, int dealer, int vul, object[]? reuse = null)
{
    var colon = pbn.IndexOf(':'); int first = "NESW".IndexOf(pbn[colon - 1]);
    var hands = new string[4][];
    var parts = pbn[(colon + 1)..].Split(' ', StringSplitOptions.RemoveEmptyEntries);
    for (int i = 0; i < 4; i++) { var s = parts[i].Split('.'); hands[(first + i) % 4] = [s[3], s[2], s[1], s[0]]; }
    var bots = reuse ?? NewSet();
    for (int i = 0; i < 4; i++)
    {
        object[] a = [i, hands[i], dealer, vul, false, false];
        newHand.Invoke(bots[i], a);
        setScoring.Invoke(bots[i], [0]);
        ApplyCard(bots[i], cards[0], 0); ApplyCard(bots[i], cards[1], 1);
    }
    var seq = new List<int>(); int pos = dealer, passes = 0; bool hasBid = false;
    for (int r = 0; r < 100; r++)
    {
        int code = (int)getBid.Invoke(bots[pos], null)!;
        foreach (var b in bots) setBid.Invoke(b, [pos, code, ""]);
        seq.Add(code);
        if (code == 0) { passes++; if ((hasBid && passes >= 3) || (!hasBid && passes >= 4)) break; }
        else { passes = 0; if (code >= 5) hasBid = true; }
        pos = (pos + 1) % 4;
    }
    return string.Join(" ", seq);
}

// Reference: strictly sequential.
var sw = Stopwatch.StartNew();
var reference = deals.Select(d => { try { return Bid(d.pbn, d.dealer, d.vul); } catch (Exception e) { return "EXC " + (e.InnerException ?? e).GetType().Name; } }).ToArray();
Console.WriteLine($"Sequential: {sw.ElapsedMilliseconds} ms, {reference.Count(r => r.StartsWith("EXC"))} exceptions");

if (pool)
{
    // All instances are created here, before any bidding; afterwards nothing calls the constructor.
    for (int k = 0; k < threads; k++) sets.Add(NewSet());
    Console.WriteLine($"POOL mode: {threads} sets of 4 instances pre-created and reused");
}
string Run(int i)
{
    if (!pool) return Bid(deals[i].pbn, deals[i].dealer, deals[i].vul);
    sets.TryTake(out var set);
    try { return Bid(deals[i].pbn, deals[i].dealer, deals[i].vul, set); } finally { sets.Add(set!); }
}
// Same deals, many threads, several rounds, shuffled order so different deals overlap each time.
int mismatches = 0, exceptions = 0, total = 0; var examples = new ConcurrentBag<string>();
sw.Restart();
for (int round = 0; round < rounds; round++)
{
    var order = Enumerable.Range(0, deals.Count).OrderBy(_ => Random.Shared.Next()).ToArray();
    Parallel.ForEach(order, new ParallelOptions { MaxDegreeOfParallelism = threads }, i =>
    {
        Interlocked.Increment(ref total);
        string got;
        try { got = Run(i); }
        catch (Exception e) { Interlocked.Increment(ref exceptions); got = "EXC " + (e.InnerException ?? e).GetType().Name + ": " + (e.InnerException ?? e).Message; }
        if (got != reference[i]) { Interlocked.Increment(ref mismatches); if (examples.Count < 5) examples.Add($"deal {i}: seq [{reference[i]}] vs par [{got}]"); }
    });
}
Console.WriteLine($"Parallel x{threads}, {rounds} rounds: {sw.ElapsedMilliseconds} ms, {total} auctions, {mismatches} differ from sequential, {exceptions} exceptions");
foreach (var e in examples) Console.WriteLine("  " + e);
