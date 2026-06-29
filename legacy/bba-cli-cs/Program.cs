using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.IO.Compression;
using System.Net.Http;
using System.Reflection;
using System.Text.RegularExpressions;
using System.Threading.Tasks;

namespace BbaCli
{
    /// <summary>
    /// BBA CLI - Bridge Bidding Analyzer Command Line Interface
    /// Directly uses EPBot64.dll for auction generation (no subprocess overhead).
    /// </summary>
    class Program
    {
        public static string Version => GetVersionFromAssembly();

        private static string GetVersionFromAssembly()
        {
            var assembly = Assembly.GetExecutingAssembly();
            // Try to get InformationalVersion first (includes full version string)
            var infoVersionAttr = assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>();
            if (infoVersionAttr != null && !string.IsNullOrEmpty(infoVersionAttr.InformationalVersion))
            {
                // Strip any +hash suffix that might be added
                var version = infoVersionAttr.InformationalVersion;
                var plusIndex = version.IndexOf('+');
                return plusIndex > 0 ? version.Substring(0, plusIndex) : version;
            }
            // Fall back to assembly version
            return assembly.GetName().Version?.ToString() ?? "0.0.0";
        }

        static int Main(string[] args)
        {
            try
            {
                string inputPath = null;
                string outputPath = null;
                string nsConventions = null;
                string ewConventions = null;
                string scoring = "MP";  // Default to Matchpoints
                bool verbose = false;
                bool dryRun = false;
                bool autoUpdate = false;
                bool noAutoUpdate = false;

                // Parse arguments (matching Rust CLI)
                for (int i = 0; i < args.Length; i++)
                {
                    switch (args[i])
                    {
                        case "-i":
                        case "--input":
                            inputPath = args[++i];
                            break;
                        case "-o":
                        case "--output":
                            outputPath = args[++i];
                            break;
                        case "--ns-conventions":
                            nsConventions = args[++i];
                            break;
                        case "--ew-conventions":
                            ewConventions = args[++i];
                            break;
                        case "-v":
                        case "--verbose":
                            verbose = true;
                            break;
                        case "--dry-run":
                            dryRun = true;
                            break;
                        case "--scoring":
                            scoring = args[++i];
                            break;
                        case "--auto-update":
                            autoUpdate = true;
                            break;
                        case "--no-auto-update":
                            noAutoUpdate = true;
                            break;
                        case "-j":
                        case "--threads":
                            i++; // Skip value - not implemented but accept for compatibility
                            break;
                        case "--wrapper":
                            i++; // Skip value - not needed in C# version
                            break;
                        case "-h":
                        case "--help":
                            PrintUsage();
                            return 0;
                        case "--version":
                            Console.WriteLine($"bba-cli {Version}");
                            return 0;
                    }
                }

                // Handle auto-update if requested and not suppressed
                if (autoUpdate && !noAutoUpdate)
                {
                    var updater = new AutoUpdater { Verbose = verbose };
                    var updateResult = updater.CheckAndUpdate(args);
                    if (updateResult == AutoUpdater.UpdateResult.Updated)
                    {
                        // Update succeeded and new process completed - return its exit code
                        return updater.RestartExitCode;
                    }
                    // UpdateResult.NoUpdate or UpdateResult.Error - continue with normal processing
                }

                // Validate required arguments
                if (string.IsNullOrEmpty(inputPath))
                {
                    Console.Error.WriteLine("Error: --input is required");
                    PrintUsage();
                    return 1;
                }
                if (string.IsNullOrEmpty(outputPath))
                {
                    Console.Error.WriteLine("Error: --output is required");
                    PrintUsage();
                    return 1;
                }
                if (string.IsNullOrEmpty(nsConventions))
                {
                    Console.Error.WriteLine("Error: --ns-conventions is required");
                    PrintUsage();
                    return 1;
                }
                if (string.IsNullOrEmpty(ewConventions))
                {
                    Console.Error.WriteLine("Error: --ew-conventions is required");
                    PrintUsage();
                    return 1;
                }

                // Validate files exist
                if (!File.Exists(inputPath))
                {
                    Console.Error.WriteLine($"Error: Input file not found: {inputPath}");
                    return 1;
                }
                if (!File.Exists(nsConventions))
                {
                    Console.Error.WriteLine($"Error: NS conventions file not found: {nsConventions}");
                    return 1;
                }
                if (!File.Exists(ewConventions))
                {
                    Console.Error.WriteLine($"Error: EW conventions file not found: {ewConventions}");
                    return 1;
                }

                if (verbose)
                {
                    Console.Error.WriteLine($"BBA-CLI v{Version}");
                    Console.Error.WriteLine($"Input: {inputPath}");
                    Console.Error.WriteLine($"Output: {outputPath}");
                    Console.Error.WriteLine($"NS Conventions: {nsConventions}");
                    Console.Error.WriteLine($"EW Conventions: {ewConventions}");
                    Console.Error.WriteLine($"Scoring: {scoring}");
                }

                var processor = new PbnProcessor { Verbose = verbose, Scoring = scoring };
                var stats = processor.ProcessFile(inputPath, outputPath, nsConventions, ewConventions, dryRun);

                Console.Error.WriteLine($"Processed {stats.DealsProcessed} deals, generated {stats.AuctionsGenerated} auctions");
                if (stats.Errors > 0)
                {
                    Console.Error.WriteLine($"{stats.Errors} deals had errors");
                }

                if (dryRun)
                {
                    Console.Error.WriteLine("Dry run complete - no output written");
                }
                else if (verbose)
                {
                    Console.Error.WriteLine($"Output written to {outputPath}");
                }

                return stats.Errors > 0 ? 1 : 0;
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"Error: {ex.Message}");
                return 1;
            }
        }

        static void PrintUsage()
        {
            Console.WriteLine($"BBA-CLI v{Version} - Bridge Bidding Analyzer");
            Console.WriteLine();
            Console.WriteLine("Generates bridge auctions for deals in PBN files using the EPBot engine.");
            Console.WriteLine();
            Console.WriteLine("Usage: bba-cli --input <FILE> --output <FILE> --ns-conventions <FILE> --ew-conventions <FILE>");
            Console.WriteLine();
            Console.WriteLine("Options:");
            Console.WriteLine("  -i, --input <FILE>           Input PBN file containing deals to analyze");
            Console.WriteLine("  -o, --output <FILE>          Output PBN file for results with generated auctions");
            Console.WriteLine("  --ns-conventions <FILE>      Convention file (.bbsa) for North-South partnership");
            Console.WriteLine("  --ew-conventions <FILE>      Convention file (.bbsa) for East-West partnership");
            Console.WriteLine("  -v, --verbose                Enable verbose logging");
            Console.WriteLine("  --dry-run                    Parse input but don't write output");
            Console.WriteLine("  --scoring <TYPE>             Scoring type: MP (default), IMP, BAM, etc.");
            Console.WriteLine("  --auto-update                Check for updates (daily) and install if available");
            Console.WriteLine("  --no-auto-update             Skip update check (used internally after update)");
            Console.WriteLine("  -j, --threads <N>            (ignored, for compatibility)");
            Console.WriteLine("  --wrapper <FILE>             (ignored, for compatibility)");
            Console.WriteLine("  -h, --help                   Show this help");
            Console.WriteLine("  --version                    Show version");
        }
    }

    public class ProcessingStats
    {
        public int DealsProcessed { get; set; }
        public int AuctionsGenerated { get; set; }
        public int Errors { get; set; }
    }

    /// <summary>
    /// Processes PBN files using EPBot64 directly (no subprocess).
    /// </summary>
    public class PbnProcessor
    {
        private const int C_NORTH = 0;
        private const int C_EAST = 1;
        private const int C_SOUTH = 2;
        private const int C_WEST = 3;

        public bool Verbose { get; set; }
        public string Scoring { get; set; } = "MP";

        public ProcessingStats ProcessFile(string inputPath, string outputPath, string nsConventions, string ewConventions, bool dryRun = false)
        {
            var stats = new ProcessingStats();
            var pbnFile = ReadPbnFile(inputPath);

            if (Verbose)
                Console.Error.WriteLine($"Read {pbnFile.Games.Count} games from {inputPath}");

            // Process each game
            foreach (var game in pbnFile.Games)
            {
                if (game.Deal == null) continue;

                stats.DealsProcessed++;

                try
                {
                    var auction = GenerateAuction(game, nsConventions, ewConventions);
                    game.Auction = auction;
                    ComputeContractFromAuction(game);
                    stats.AuctionsGenerated++;
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine($"Error processing game {stats.DealsProcessed}: {ex.Message}");
                    if (Verbose)
                        Console.Error.WriteLine($"  Stack trace: {ex.StackTrace}");
                    stats.Errors++;
                }
            }

            // Write output
            if (!dryRun)
            {
                WritePbnFile(outputPath, pbnFile, nsConventions, ewConventions, inputPath);
                if (Verbose)
                    Console.Error.WriteLine($"Wrote {pbnFile.Games.Count} games to {outputPath}");
            }

            return stats;
        }

        /// <summary>
        /// Generate auction for a single game using 4 EPBot instances.
        /// </summary>
        private List<string> GenerateAuction(PbnGame game, string nsConventions, string ewConventions)
        {
            int dealer = game.Dealer;
            int vul = game.Vulnerability;
            string[][] hands = game.Deal;

            // Create 4 EPBot instances (direct reference, no reflection!)
            EPBot64.EPBot[] players = new EPBot64.EPBot[4];

            for (int i = 0; i < 4; i++)
            {
                players[i] = new EPBot64.EPBot();

                // Set up the hand
                string[] hand = hands[i];
                players[i].new_hand(i, ref hand, dealer, vul, false, false);

                // Set scoring mode (1 = IMP)
                players[i].scoring = 1;

                // Load conventions
                if (!string.IsNullOrEmpty(nsConventions) && File.Exists(nsConventions))
                {
                    LoadConventions(players[i], nsConventions, 0);
                }
                if (!string.IsNullOrEmpty(ewConventions) && File.Exists(ewConventions))
                {
                    LoadConventions(players[i], ewConventions, 1);
                }
            }

            // Generate auction
            var bids = new List<string>();
            var alerts = new List<string>();
            int alertIndex = 0;
            int currentPos = dealer;
            int passCount = 0;
            bool hasBid = false;

            for (int round = 0; round < 100; round++) // Safety limit
            {
                // Get bid from current player
                int bidCode = players[currentPos].get_bid();
                string bidStr = DecodeBid(bidCode);

                // Broadcast to all players
                for (int i = 0; i < 4; i++)
                {
                    players[i].set_bid(currentPos, bidCode);
                }

                // Check for alert (VB.NET indexed properties accessed via get_ methods in C#)
                try
                {
                    // Note: info_alerting/info_meaning use the position of the PARTNER who would alert
                    // For positions N=0,E=1,S=2,W=3, partner is (pos+2)%4
                    int partnerPos = (currentPos + 2) % 4;
                    bool isAlert = players[partnerPos].get_info_alerting(currentPos);
                    if (isAlert)
                    {
                        alertIndex++;
                        string meaning = players[partnerPos].get_info_meaning(currentPos);
                        if (!string.IsNullOrEmpty(meaning))
                        {
                            bidStr = $"{bidStr} ={alertIndex}=";
                            alerts.Add($"{alertIndex}:{meaning}");
                        }
                    }
                }
                catch
                {
                    // Alert methods may not be available in all versions or may fail for some bids
                }

                bids.Add(bidStr);

                // Track passes
                if (bidStr == "Pass" || bidStr.StartsWith("Pass "))
                {
                    passCount++;
                }
                else
                {
                    passCount = 0;
                    hasBid = true;
                }

                // Auction ends with 3 passes after a bid, or 4 initial passes
                if ((hasBid && passCount >= 3) || (!hasBid && passCount >= 4))
                {
                    break;
                }

                currentPos = (currentPos + 1) % 4;
            }

            game.AlertNotes = alerts;
            return bids;
        }

        /// <summary>
        /// Compute the final contract and declarer from the auction.
        /// </summary>
        private void ComputeContractFromAuction(PbnGame game)
        {
            if (game.Auction == null || game.Auction.Count == 0)
            {
                game.Contract = "Pass";
                game.Declarer = -1;
                return;
            }

            string lastBid = null;
            int lastBidder = -1;
            bool doubled = false;
            bool redoubled = false;
            int currentPos = game.Dealer;

            // Track first bidder of each strain for each partnership
            // firstBid[side][strain] where side 0=NS, 1=EW and strain 0-4=C,D,H,S,NT
            int[,] firstBidOfStrain = new int[2, 5];
            for (int s = 0; s < 2; s++)
                for (int st = 0; st < 5; st++)
                    firstBidOfStrain[s, st] = -1;

            foreach (var bidWithAlert in game.Auction)
            {
                // Strip alert markers like " =1=" from the bid
                string bid = bidWithAlert;
                int alertPos = bid.IndexOf(" =");
                if (alertPos > 0)
                    bid = bid.Substring(0, alertPos);

                if (bid == "Pass")
                {
                    // Do nothing
                }
                else if (bid == "X")
                {
                    doubled = true;
                    redoubled = false;
                }
                else if (bid == "XX")
                {
                    redoubled = true;
                }
                else if (bid.Length >= 2)
                {
                    // A real bid like "1NT", "3S", etc.
                    lastBid = bid;
                    lastBidder = currentPos;
                    doubled = false;
                    redoubled = false;

                    // Determine the strain
                    int strain = GetStrainFromBid(bid);
                    if (strain >= 0)
                    {
                        int side = currentPos % 2;  // 0 for NS, 1 for EW
                        if (firstBidOfStrain[side, strain] < 0)
                        {
                            firstBidOfStrain[side, strain] = currentPos;
                        }
                    }
                }

                currentPos = (currentPos + 1) % 4;
            }

            // Determine final contract
            if (lastBid == null)
            {
                // All pass
                game.Contract = "Pass";
                game.Declarer = -1;
            }
            else
            {
                // Get the strain of the final contract
                int strain = GetStrainFromBid(lastBid);
                int declaringSide = lastBidder % 2;

                // Declarer is first person on that side to bid this strain
                game.Declarer = firstBidOfStrain[declaringSide, strain];

                // Build contract string
                string contractStr = lastBid.Replace("NT", "N");
                if (redoubled)
                    contractStr += "XX";
                else if (doubled)
                    contractStr += "X";

                game.Contract = contractStr;
            }
        }

        private int GetStrainFromBid(string bid)
        {
            if (bid.Length < 2) return -1;
            string strain = bid.Substring(1).ToUpper();
            switch (strain)
            {
                case "C": return 0;
                case "D": return 1;
                case "H": return 2;
                case "S": return 3;
                case "N":
                case "NT": return 4;
                default: return -1;
            }
        }

        /// <summary>
        /// Load conventions from a .bbsa file.
        /// </summary>
        private void LoadConventions(EPBot64.EPBot bot, string filePath, int side)
        {
            foreach (var line in File.ReadAllLines(filePath))
            {
                if (string.IsNullOrWhiteSpace(line) || line.StartsWith("#") || line.StartsWith(";"))
                    continue;

                var parts = line.Split(new[] { '=' }, 2);
                if (parts.Length != 2) continue;

                string key = parts[0].Trim();
                string valueStr = parts[1].Trim();

                if (int.TryParse(valueStr, out int intValue))
                {
                    if (key.Equals("System type", StringComparison.OrdinalIgnoreCase))
                    {
                        bot.set_system_type(side, intValue);
                    }
                    else if (key.Equals("Opponent type", StringComparison.OrdinalIgnoreCase))
                    {
                        bot.set_opponent_type(side, intValue);
                    }
                    else
                    {
                        // Treat 0/1 as boolean for convention settings
                        try
                        {
                            bot.set_conventions(side, key, intValue != 0);
                        }
                        catch { /* Ignore unknown conventions */ }
                    }
                }
                else if (bool.TryParse(valueStr, out bool boolValue))
                {
                    // Also support "True"/"False" string values
                    try
                    {
                        bot.set_conventions(side, key, boolValue);
                    }
                    catch { /* Ignore unknown conventions */ }
                }
            }
        }

        /// <summary>
        /// Decode EPBot bid code to string.
        /// </summary>
        private string DecodeBid(int code)
        {
            if (code == 0) return "Pass";
            if (code == 1) return "X";
            if (code == 2) return "XX";

            if (code >= 5 && code <= 39)
            {
                int adjustedCode = code - 5;
                int level = adjustedCode / 5 + 1;
                int suit = adjustedCode % 5;
                string[] suits = { "C", "D", "H", "S", "NT" };
                return $"{level}{suits[suit]}";
            }

            return $"?{code}";
        }

        #region PBN File I/O

        private PbnFile ReadPbnFile(string path)
        {
            var pbnFile = new PbnFile();
            PbnGame current = null;
            bool inAuction = false;
            bool headerDone = false;
            string lastTagName = null;

            foreach (var line in File.ReadAllLines(path))
            {
                string trimmed = line.Trim();

                // Blank line ends current game
                if (string.IsNullOrEmpty(trimmed))
                {
                    if (current != null && current.Tags.Count > 0)
                    {
                        pbnFile.Games.Add(current);
                        current = null;
                        inAuction = false;
                        lastTagName = null;
                    }
                    continue;
                }

                // Header comments (% lines before any games)
                if (trimmed.StartsWith("%") && !headerDone)
                {
                    pbnFile.HeaderComments.Add(line);
                    continue;
                }

                // Tag line
                if (trimmed.StartsWith("[") && trimmed.EndsWith("]"))
                {
                    headerDone = true;
                    if (current == null)
                        current = new PbnGame();

                    var (name, value) = ParseTag(trimmed);
                    lastTagName = name;

                    // Skip existing Auction tag - we'll generate our own
                    if (name.Equals("Auction", StringComparison.OrdinalIgnoreCase))
                    {
                        current.HasExistingAuction = true;
                        inAuction = true;
                        continue;
                    }

                    // Skip tags that we'll regenerate with the new auction
                    if (name.Equals("Note", StringComparison.OrdinalIgnoreCase) ||
                        name.Equals("Scoring", StringComparison.OrdinalIgnoreCase) ||
                        name.Equals("Play", StringComparison.OrdinalIgnoreCase) ||
                        name.StartsWith("BidSystem", StringComparison.OrdinalIgnoreCase))
                    {
                        continue;
                    }

                    current.Tags.Add((name, value));
                    inAuction = false;

                    if (name.Equals("Deal", StringComparison.OrdinalIgnoreCase))
                    {
                        current.Deal = ParseDeal(value, out int firstSeat);
                    }
                    else if (name.Equals("Dealer", StringComparison.OrdinalIgnoreCase))
                    {
                        current.Dealer = ParsePosition(value);
                    }
                    else if (name.Equals("Vulnerable", StringComparison.OrdinalIgnoreCase))
                    {
                        current.Vulnerability = ParseVulnerability(value);
                    }
                }
                // Comment line
                else if (trimmed.StartsWith("{") && trimmed.EndsWith("}"))
                {
                    if (current != null)
                    {
                        // Classify the comment
                        if (trimmed.StartsWith("{Shape"))
                            current.ShapeComment = trimmed;
                        else if (trimmed.StartsWith("{HCP"))
                            current.HcpComment = trimmed;
                        else if (trimmed.StartsWith("{Losers"))
                            current.LosersComment = trimmed;
                        else if (lastTagName != null && lastTagName.Equals("Board", StringComparison.OrdinalIgnoreCase))
                            current.HexComment = trimmed;  // Hex comment comes after Board tag
                        else
                            current.OtherLines.Add(line);
                    }
                }
                // Auction bids (skip - we'll generate our own)
                else if (inAuction)
                {
                    // Skip existing auction lines
                    continue;
                }
                // Other content (skip play section placeholders like "*")
                else if (current != null && trimmed != "*")
                {
                    current.OtherLines.Add(line);
                }
            }

            if (current != null && current.Tags.Count > 0)
            {
                pbnFile.Games.Add(current);
            }

            return pbnFile;
        }

        private void WritePbnFile(string path, PbnFile pbnFile, string nsConventions, string ewConventions, string inputPath)
        {
            using (var writer = new StreamWriter(path))
            {
                // Filter out any previous BBA-CLI generated header comments
                // (lines starting with "% PBN", "% Generated by BBA", "% https://github.com/bridge-craftwork/BBA-Tools",
                // "% The source file name:", "% CC1 -", "% CC2 -", "% 1-2 -")
                var preservedComments = new List<string>();
                foreach (var comment in pbnFile.HeaderComments)
                {
                    string trimmed = comment.TrimStart();
                    if (trimmed.StartsWith("% PBN ") ||
                        trimmed.StartsWith("% Generated by BBA") ||
                        trimmed.StartsWith("% https://github.com/bridge-craftwork/BBA-Tools") ||
                        trimmed.StartsWith("% The source file name:") ||
                        trimmed.StartsWith("% CC1 -") ||
                        trimmed.StartsWith("% CC2 -") ||
                        trimmed.StartsWith("% 1-2 -"))
                    {
                        // Skip BBA-CLI generated comments - we'll regenerate them
                        continue;
                    }
                    preservedComments.Add(comment);
                }

                // Write preserved non-BBA header comments first
                foreach (var comment in preservedComments)
                {
                    writer.WriteLine(comment);
                }

                // Always generate our standard BBA-CLI header comments
                writer.WriteLine("% PBN 2.1");
                writer.WriteLine($"% Generated by BBA-CLI v{Program.Version}");
                writer.WriteLine("% https://github.com/bridge-craftwork/BBA-Tools");
                writer.WriteLine($"% The source file name: \"{Path.GetFullPath(inputPath)}\"");
                writer.WriteLine($"% CC1 - {nsConventions}");
                writer.WriteLine($"% CC2 - {ewConventions}");

                // Read and compare conventions from both files
                var cc1Conventions = ReadEnabledConventions(nsConventions);
                var cc2Conventions = ReadEnabledConventions(ewConventions);

                // Find conventions in both, CC1 only, CC2 only
                foreach (var conv in cc1Conventions)
                {
                    if (cc2Conventions.Contains(conv))
                        writer.WriteLine($"% 1-2 - {conv}");
                    else
                        writer.WriteLine($"% CC1 - {conv}");
                }
                foreach (var conv in cc2Conventions)
                {
                    if (!cc1Conventions.Contains(conv))
                        writer.WriteLine($"% CC2 - {conv}");
                }

                // Blank line after header
                writer.WriteLine();

                for (int i = 0; i < pbnFile.Games.Count; i++)
                {
                    if (i > 0) writer.WriteLine();

                    var game = pbnFile.Games[i];

                    // Write tags with comments in correct positions
                    bool wrotePlayers = false;

                    foreach (var (name, value) in game.Tags)
                    {
                        // Skip individual player tags - we'll write them in correct order
                        if (name.Equals("North", StringComparison.OrdinalIgnoreCase) ||
                            name.Equals("East", StringComparison.OrdinalIgnoreCase) ||
                            name.Equals("South", StringComparison.OrdinalIgnoreCase) ||
                            name.Equals("West", StringComparison.OrdinalIgnoreCase))
                        {
                            continue;
                        }

                        // Write player tags in correct order (N, E, S, W) before Dealer
                        if (name.Equals("Dealer", StringComparison.OrdinalIgnoreCase) && !wrotePlayers)
                        {
                            writer.WriteLine("[North \"EPBot\"]");
                            writer.WriteLine("[East \"EPBot\"]");
                            writer.WriteLine("[South \"EPBot\"]");
                            writer.WriteLine("[West \"EPBot\"]");
                            writer.WriteLine("[Room \"Open\"]");
                            wrotePlayers = true;
                        }

                        // Convert Site "-" to Site ""
                        if (name.Equals("Site", StringComparison.OrdinalIgnoreCase))
                        {
                            string siteValue = (value == "-") ? "" : value;
                            writer.WriteLine($"[Site \"{siteValue}\"]");
                            continue;
                        }

                        // Update Declarer based on computed value
                        if (name.Equals("Declarer", StringComparison.OrdinalIgnoreCase))
                        {
                            string declarer = game.Declarer >= 0 ? new[] { "N", "E", "S", "W" }[game.Declarer] : "";
                            writer.WriteLine($"[Declarer \"{declarer}\"]");
                            continue;
                        }

                        // Update Contract based on computed value
                        if (name.Equals("Contract", StringComparison.OrdinalIgnoreCase))
                        {
                            writer.WriteLine($"[Contract \"{game.Contract ?? ""}\"]");
                            continue;
                        }

                        // Skip Result - we can't compute it without double-dummy
                        if (name.Equals("Result", StringComparison.OrdinalIgnoreCase))
                        {
                            writer.WriteLine($"[Result \"\"]");
                            continue;
                        }

                        writer.WriteLine($"[{name} \"{value}\"]");

                        // Hex comment goes immediately after Board tag
                        if (name.Equals("Board", StringComparison.OrdinalIgnoreCase) && !string.IsNullOrEmpty(game.HexComment))
                        {
                            writer.WriteLine(game.HexComment);
                        }

                        // Shape/HCP/Losers comments go after Deal tag
                        if (name.Equals("Deal", StringComparison.OrdinalIgnoreCase))
                        {
                            if (!string.IsNullOrEmpty(game.ShapeComment))
                                writer.WriteLine(game.ShapeComment);
                            if (!string.IsNullOrEmpty(game.HcpComment))
                                writer.WriteLine(game.HcpComment);
                            if (!string.IsNullOrEmpty(game.LosersComment))
                                writer.WriteLine(game.LosersComment);
                        }
                    }

                    // Add Scoring tag before Auction
                    writer.WriteLine($"[Scoring \"{Scoring}\"]");

                    // Write auction if generated
                    if (game.Auction != null && game.Auction.Count > 0)
                    {
                        string dealerChar = new[] { "N", "E", "S", "W" }[game.Dealer];
                        writer.WriteLine($"[Auction \"{dealerChar}\"]");
                        writer.WriteLine(FormatAuction(game.Auction));
                    }

                    // Write alert notes (after auction, before play)
                    if (game.AlertNotes != null && game.AlertNotes.Count > 0)
                    {
                        foreach (var note in game.AlertNotes)
                        {
                            writer.WriteLine($"[Note \"{note}\"]");
                        }
                    }

                    // Add Play tag (empty - we don't generate play)
                    if (game.Declarer >= 0)
                    {
                        // Leader is player to the left of declarer
                        string leader = new[] { "N", "E", "S", "W" }[(game.Declarer + 1) % 4];
                        writer.WriteLine($"[Play \"{leader}\"]");
                        writer.WriteLine("*");
                    }

                    // Write other lines
                    foreach (var line in game.OtherLines)
                    {
                        writer.WriteLine(line);
                    }

                    // Add BidSystem tags at the end
                    writer.WriteLine($"[BidSystemEW \"{GetBidSystemName(ewConventions)}\"]");
                    writer.WriteLine($"[BidSystemNS \"{GetBidSystemName(nsConventions)}\"]");
                }
            }
        }

        private string GetBidSystemName(string conventionFilePath)
        {
            string fileName = Path.GetFileNameWithoutExtension(conventionFilePath);
            // Convert common convention file names to readable names
            if (fileName.Contains("21GF") || fileName.Contains("2-1"))
                return "2/1GF - 2/1 Game Force";
            if (fileName.Contains("SAYC"))
                return "SAYC - Standard American Yellow Card";
            if (fileName.Contains("Precision"))
                return "Precision";
            return fileName;
        }

        /// <summary>
        /// Read enabled conventions (value = 1 or True) from a .bbsa file.
        /// </summary>
        private HashSet<string> ReadEnabledConventions(string filePath)
        {
            var conventions = new HashSet<string>();
            if (!File.Exists(filePath)) return conventions;

            foreach (var line in File.ReadAllLines(filePath))
            {
                if (string.IsNullOrWhiteSpace(line) || line.StartsWith("#") || line.StartsWith(";"))
                    continue;

                var parts = line.Split(new[] { '=' }, 2);
                if (parts.Length != 2) continue;

                string key = parts[0].Trim();
                string valueStr = parts[1].Trim();

                // Skip system type and opponent type - they're not conventions
                if (key.Equals("System type", StringComparison.OrdinalIgnoreCase) ||
                    key.Equals("Opponent type", StringComparison.OrdinalIgnoreCase))
                    continue;

                // Check if convention is enabled (value is 1 or True)
                if (valueStr == "1" || valueStr.Equals("True", StringComparison.OrdinalIgnoreCase))
                {
                    conventions.Add(key);
                }
            }

            return conventions;
        }

        private (string name, string value) ParseTag(string line)
        {
            string inner = line.TrimStart('[').TrimEnd(']');
            int space = inner.IndexOf(' ');
            if (space < 0) return (inner, "");

            string name = inner.Substring(0, space);
            string value = inner.Substring(space + 1).Trim('"');
            return (name, value);
        }

        /// <summary>
        /// Parse PBN deal. Returns hands[4][4] where hands[position][suit].
        /// PBN format is S.H.D.C but EPBot expects C.D.H.S, so we reverse.
        /// </summary>
        private string[][] ParseDeal(string deal, out int firstSeat)
        {
            int colonPos = deal.IndexOf(':');
            firstSeat = ParsePosition(deal.Substring(colonPos - 1, 1));

            string handsStr = deal.Substring(colonPos + 1).Trim();
            string[] handParts = handsStr.Split(new[] { ' ' }, StringSplitOptions.RemoveEmptyEntries);

            string[][] hands = new string[4][];
            for (int i = 0; i < 4; i++)
            {
                int pos = (firstSeat + i) % 4;
                // Split by '.' but don't remove empty entries - empty suits are valid
                string[] suits = handParts[i].Split('.');
                // Ensure we have exactly 4 suits
                if (suits.Length != 4)
                {
                    throw new Exception($"Invalid hand format: {handParts[i]} has {suits.Length} suits instead of 4");
                }
                Array.Reverse(suits); // PBN S.H.D.C -> EPBot C.D.H.S
                hands[pos] = suits;
            }

            return hands;
        }

        private int ParsePosition(string s)
        {
            switch (s.Trim().ToUpper())
            {
                case "N": return C_NORTH;
                case "E": return C_EAST;
                case "S": return C_SOUTH;
                case "W": return C_WEST;
                default: return C_NORTH;
            }
        }

        private int ParseVulnerability(string s)
        {
            switch (s.Trim().ToUpper())
            {
                case "NONE":
                case "-":
                    return 0;
                case "NS":
                case "N-S":
                    return 2;
                case "EW":
                case "E-W":
                    return 1;
                case "BOTH":
                case "ALL":
                    return 3;
                default:
                    return 0;
            }
        }

        private string FormatAuction(List<string> bids)
        {
            var lines = new List<string>();
            for (int i = 0; i < bids.Count; i += 4)
            {
                int count = Math.Min(4, bids.Count - i);
                lines.Add(string.Join(" ", bids.GetRange(i, count)));
            }
            return string.Join("\n", lines);
        }

        #endregion
    }

    public class PbnGame
    {
        public List<(string Name, string Value)> Tags { get; } = new List<(string, string)>();
        public List<string> OtherLines { get; } = new List<string>();
        public string[][] Deal { get; set; }
        public int Dealer { get; set; }
        public int Vulnerability { get; set; }
        public List<string> Auction { get; set; }

        // Comments and their positions
        public string HexComment { get; set; }  // The {089EE...} comment after Board tag
        public string ShapeComment { get; set; }  // {Shape ...}
        public string HcpComment { get; set; }    // {HCP ...}
        public string LosersComment { get; set; } // {Losers ...}

        // Existing auction from input (to be replaced)
        public List<string> OriginalAuction { get; set; }
        public bool HasExistingAuction { get; set; }

        // Contract info computed from auction
        public string Contract { get; set; }  // e.g., "3S", "4HX", "Pass"
        public int Declarer { get; set; } = -1;  // 0=N, 1=E, 2=S, 3=W, -1=unknown

        // Alert notes for the auction
        public List<string> AlertNotes { get; set; } = new List<string>();
    }

    public class PbnFile
    {
        public List<string> HeaderComments { get; } = new List<string>();  // % lines at start
        public List<PbnGame> Games { get; } = new List<PbnGame>();
    }

    /// <summary>
    /// Handles automatic updates from GitHub releases.
    /// </summary>
    public class AutoUpdater
    {
        private const string GitHubApiUrl = "https://api.github.com/repos/bridge-craftwork/BBA-Tools/releases/latest";
        private const string AppName = "BBA-CLI";
        private const int TimeoutSeconds = 10;

        public bool Verbose { get; set; }

        public enum UpdateResult
        {
            NoUpdate,   // No update available or check skipped
            Updated,    // Update installed, new process completed
            Error       // Error occurred (silently continue)
        }

        /// <summary>
        /// Exit code from the restarted process after update (only valid if UpdateResult.Updated)
        /// </summary>
        public int RestartExitCode { get; private set; }

        /// <summary>
        /// Check for updates and install if available.
        /// </summary>
        public UpdateResult CheckAndUpdate(string[] originalArgs)
        {
            try
            {
                // Check if we should check for updates (daily check)
                if (!ShouldCheckForUpdates())
                {
                    if (Verbose)
                        Console.Error.WriteLine("Update check skipped (checked within last 24 hours)");
                    return UpdateResult.NoUpdate;
                }

                if (Verbose)
                    Console.Error.WriteLine("Checking for updates...");

                // Get latest release info from GitHub
                var releaseInfo = GetLatestReleaseAsync().GetAwaiter().GetResult();
                if (releaseInfo == null)
                {
                    if (Verbose)
                        Console.Error.WriteLine("Could not retrieve release info");
                    return UpdateResult.Error;
                }

                // Update the last-check timestamp
                UpdateLastCheckTimestamp();

                // Compare versions
                string currentVersion = Program.Version;
                string latestVersion = releaseInfo.TagName.TrimStart('v');

                if (Verbose)
                {
                    Console.Error.WriteLine($"Current version: {currentVersion}");
                    Console.Error.WriteLine($"Latest version: {latestVersion}");
                }

                if (!IsNewerVersion(latestVersion, currentVersion))
                {
                    if (Verbose)
                        Console.Error.WriteLine("Already up to date");
                    return UpdateResult.NoUpdate;
                }

                // Download and install update
                if (Verbose)
                    Console.Error.WriteLine($"Downloading update {releaseInfo.TagName}...");

                string downloadUrl = releaseInfo.ZipUrl;
                if (string.IsNullOrEmpty(downloadUrl))
                {
                    if (Verbose)
                        Console.Error.WriteLine("No download URL found in release");
                    return UpdateResult.Error;
                }

                string tempDir = Path.Combine(Path.GetTempPath(), $"bba-cli-update-{releaseInfo.TagName}");
                string zipPath = Path.Combine(tempDir, "update.zip");
                string extractPath = Path.Combine(tempDir, "extracted");

                // Clean up any previous update attempt
                if (Directory.Exists(tempDir))
                    Directory.Delete(tempDir, true);
                Directory.CreateDirectory(tempDir);
                Directory.CreateDirectory(extractPath);

                // Download the zip file
                if (!DownloadFileAsync(downloadUrl, zipPath).GetAwaiter().GetResult())
                {
                    if (Verbose)
                        Console.Error.WriteLine("Download failed");
                    return UpdateResult.Error;
                }

                if (Verbose)
                    Console.Error.WriteLine("Extracting...");

                // Extract the zip
                ZipFile.ExtractToDirectory(zipPath, extractPath);

                // Get the current exe directory
                string currentDir = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);

                if (Verbose)
                    Console.Error.WriteLine("Replacing files...");

                // Replace all files
                var updateFiles = Directory.GetFiles(extractPath, "*", SearchOption.AllDirectories);
                foreach (var updateFile in updateFiles)
                {
                    string fileName = Path.GetFileName(updateFile);
                    string currentFile = Path.Combine(currentDir, fileName);
                    string oldFile = currentFile + ".old";

                    // Delete previous .old if exists
                    if (File.Exists(oldFile))
                    {
                        try { File.Delete(oldFile); } catch { }
                    }

                    // Rename current to .old (works even for running exe on Windows)
                    if (File.Exists(currentFile))
                    {
                        if (Verbose)
                            Console.Error.WriteLine($"  {fileName} -> {fileName}.old");
                        File.Move(currentFile, oldFile);
                    }

                    // Copy new file
                    File.Copy(updateFile, currentFile);
                }

                // Write update log
                WriteUpdateLog(currentVersion, latestVersion);

                if (Verbose)
                    Console.Error.WriteLine("Update complete, restarting...");

                // Re-exec with --no-auto-update to prevent infinite loop
                var newArgs = new List<string>(originalArgs);
                newArgs.Remove("--auto-update");
                if (!newArgs.Contains("--no-auto-update"))
                    newArgs.Add("--no-auto-update");

                string exePath = Path.Combine(currentDir, "bba-cli.exe");
                var startInfo = new ProcessStartInfo
                {
                    FileName = exePath,
                    Arguments = string.Join(" ", newArgs.ConvertAll(QuoteArg)),
                    UseShellExecute = false
                };
                var process = Process.Start(startInfo);
                process.WaitForExit();
                RestartExitCode = process.ExitCode;

                // Clean up temp directory
                try { Directory.Delete(tempDir, true); } catch { }

                return UpdateResult.Updated;
            }
            catch (Exception ex)
            {
                if (Verbose)
                    Console.Error.WriteLine($"Update error: {ex.Message}");
                return UpdateResult.Error;
            }
        }

        private bool ShouldCheckForUpdates()
        {
            // Check if this is the first run since 7 AM Pacific time
            // GitHub Actions checks upstream at 6 AM Pacific and creates releases,
            // so checking after 7 AM optimizes release-to-install time

            string timestampFile = GetTimestampFilePath();
            if (!File.Exists(timestampFile))
                return true;

            try
            {
                string content = File.ReadAllText(timestampFile).Trim();
                if (DateTime.TryParse(content, out DateTime lastCheckUtc))
                {
                    // Ensure lastCheck is in UTC
                    if (lastCheckUtc.Kind != DateTimeKind.Utc)
                        lastCheckUtc = lastCheckUtc.ToUniversalTime();

                    // Get Pacific timezone (handles DST automatically)
                    TimeZoneInfo pacificZone = TimeZoneInfo.FindSystemTimeZoneById("Pacific Standard Time");

                    // Get current time in Pacific
                    DateTime nowPacific = TimeZoneInfo.ConvertTimeFromUtc(DateTime.UtcNow, pacificZone);

                    // Get today's 7 AM Pacific
                    DateTime today7amPacific = nowPacific.Date.AddHours(7);

                    // Convert last check to Pacific for comparison
                    DateTime lastCheckPacific = TimeZoneInfo.ConvertTimeFromUtc(lastCheckUtc, pacificZone);

                    // Should check if last check was before today's 7 AM Pacific
                    return lastCheckPacific < today7amPacific;
                }
            }
            catch { }

            return true;
        }

        private void UpdateLastCheckTimestamp()
        {
            try
            {
                string timestampFile = GetTimestampFilePath();
                string dir = Path.GetDirectoryName(timestampFile);
                if (!Directory.Exists(dir))
                    Directory.CreateDirectory(dir);
                File.WriteAllText(timestampFile, DateTime.UtcNow.ToString("o"));
            }
            catch { }
        }

        private string GetTimestampFilePath()
        {
            string localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            return Path.Combine(localAppData, AppName, "last-update-check.txt");
        }

        private void WriteUpdateLog(string oldVersion, string newVersion)
        {
            try
            {
                string currentDir = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
                string logPath = Path.Combine(currentDir, "auto-updated.txt");
                string entry = $"{DateTime.UtcNow:yyyy-MM-dd HH:mm:ss} UTC - Updated from v{oldVersion} to v{newVersion}\n";
                File.AppendAllText(logPath, entry);
            }
            catch { }
        }

        private async Task<ReleaseInfo> GetLatestReleaseAsync()
        {
            try
            {
                using (var client = new HttpClient())
                {
                    client.Timeout = TimeSpan.FromSeconds(TimeoutSeconds);
                    client.DefaultRequestHeaders.Add("User-Agent", $"{AppName}/{Program.Version}");
                    client.DefaultRequestHeaders.Add("Accept", "application/vnd.github.v3+json");

                    var response = await client.GetStringAsync(GitHubApiUrl);

                    // Simple JSON parsing without external dependencies
                    var tagMatch = Regex.Match(response, @"""tag_name""\s*:\s*""([^""]+)""");
                    var urlMatch = Regex.Match(response, @"""browser_download_url""\s*:\s*""([^""]+\.zip)""");

                    if (tagMatch.Success)
                    {
                        return new ReleaseInfo
                        {
                            TagName = tagMatch.Groups[1].Value,
                            ZipUrl = urlMatch.Success ? urlMatch.Groups[1].Value : null
                        };
                    }
                }
            }
            catch (Exception ex)
            {
                if (Verbose)
                    Console.Error.WriteLine($"API error: {ex.Message}");
            }

            return null;
        }

        private async Task<bool> DownloadFileAsync(string url, string path)
        {
            try
            {
                using (var client = new HttpClient())
                {
                    client.Timeout = TimeSpan.FromSeconds(60); // Longer timeout for download
                    client.DefaultRequestHeaders.Add("User-Agent", $"{AppName}/{Program.Version}");

                    var response = await client.GetAsync(url);
                    response.EnsureSuccessStatusCode();

                    using (var fs = new FileStream(path, FileMode.Create))
                    {
                        await response.Content.CopyToAsync(fs);
                    }

                    if (Verbose)
                    {
                        var fi = new FileInfo(path);
                        Console.Error.WriteLine($"Downloaded {fi.Length / 1024.0 / 1024.0:F1} MB");
                    }

                    return true;
                }
            }
            catch (Exception ex)
            {
                if (Verbose)
                    Console.Error.WriteLine($"Download error: {ex.Message}");
                return false;
            }
        }

        private bool IsNewerVersion(string latest, string current)
        {
            // Handle version formats: "8736.0" or "0.3.0"
            var latestParts = latest.Split('.');
            var currentParts = current.Split('.');

            for (int i = 0; i < Math.Max(latestParts.Length, currentParts.Length); i++)
            {
                int latestNum = i < latestParts.Length && int.TryParse(latestParts[i], out int ln) ? ln : 0;
                int currentNum = i < currentParts.Length && int.TryParse(currentParts[i], out int cn) ? cn : 0;

                if (latestNum > currentNum) return true;
                if (latestNum < currentNum) return false;
            }

            return false;
        }

        private string QuoteArg(string arg)
        {
            if (arg.Contains(" ") || arg.Contains("\""))
                return "\"" + arg.Replace("\"", "\\\"") + "\"";
            return arg;
        }

        private class ReleaseInfo
        {
            public string TagName { get; set; }
            public string ZipUrl { get; set; }
        }
    }
}
