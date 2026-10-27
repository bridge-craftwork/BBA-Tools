# EPBot thread-safety harness

These scripts check whether an `EPBotNET.dll` build bids correctly when it is called from several threads at once. bba-server runs up to `MAX_CONCURRENCY` auctions in parallel and epbot-core has no global lock, so this matters in production. The upstream issue is [EdwardPiwowar/BBA#1428](https://github.com/EdwardPiwowar/BBA/issues/1428).

They test the managed `EPBotNET.dll` (IL) rather than the native libraries in `epbot-libs/`. The native libraries are AOT-compiled from the same engine code, so a race in the IL is a race in production too.

| Script | What it does |
|--------|--------------|
| `stress.cs` | Bids the same deals once on one thread, then repeatedly on N threads, and reports any auction that differs. |
| `inspect.cs` | Static analysis: assembly version, shared (static) fields, and which methods write them. It never loads the DLL. |
| `callers.cs` | Lists the methods that call a given method, or read or write a given field. |
| `patch.cs` | Test only: makes a copy of the DLL with chosen static fields marked `[ThreadStatic]`, to confirm which field causes a race. |

## Requirements

- .NET 10 SDK. Each script is a single-file app, run with `dotnet run <script>.cs -- <args>`.
- An `EPBotNET.dll`. Builds are at the root of [Edward's repo](https://github.com/EdwardPiwowar/BBA), and candidate builds are posted as zip attachments on GitHub issues. Extract each build into its own empty directory outside this repo. Don't commit DLLs here.
- Convention cards `21GF-DEFAULT.bbsa` and `21GF-GIB.bbsa`, from the `bbsa/` folder of [Practice-Bidding-Scenarios](https://github.com/ADavidBailey/Practice-Bidding-Scenarios).

## Running

Run these from this directory. `DLL` and `BBSA` point at your local copies:

```bash
DLL=/path/to/build/EPBotNET.dll
BBSA=/path/to/Practice-Bidding-Scenarios/bbsa
FIX=../../tests/fixtures/slow

# Concurrency test: 8 threads, 3 shuffled rounds over 1,000 deals (~10 s per build)
dotnet run stress.cs -- $DLL $BBSA/21GF-DEFAULT.bbsa $BBSA/21GF-GIB.bbsa 8 3 $FIX/1N.pbn $FIX/Fourth_Suit_Forcing.pbn

# Control: 1 thread. This must report 0 differences, otherwise the engine varies between runs on its own
dotnet run stress.cs -- $DLL $BBSA/21GF-DEFAULT.bbsa $BBSA/21GF-GIB.bbsa 1 3 $FIX/1N.pbn $FIX/Fourth_Suit_Forcing.pbn

# Same test with every EPBot instance created before bidding starts
POOL=1 dotnet run stress.cs -- $DLL $BBSA/21GF-DEFAULT.bbsa $BBSA/21GF-GIB.bbsa 8 3 $FIX/1N.pbn $FIX/Fourth_Suit_Forcing.pbn

# Which shared fields does this build still write?
dotnet run inspect.cs -- $DLL
dotnet run callers.cs -- $DLL prepare_selected_positions read:index4

# Confirm a suspect: mark the named fields [ThreadStatic] in a copy, then rerun stress.cs on the copy
dotnet run patch.cs -- $DLL /path/to/patched/EPBotNET.dll index4 g_Scoring wysokosc_koncowki arr52
```

A build is thread-safe for our purposes when the 8-thread run reports **0 differing auctions and 0 exceptions**, with both `POOL=0` and `POOL=1`. The 1-thread control must also report 0. Run several rounds: a 0.4% race shows up as about 11 differences in 3,000 auctions, so a short run can miss it.

## Results so far (2026-10-26)

Wrong auctions on 8 threads:

| Build | Auctions | Wrong |
|-------|----------|-------|
| 8740 (May) | 3,000 | 1,228 (41%), plus 26 exceptions |
| 8741 candidate (posted on #1428, 2026-09-05) | 3,000 | 11 (0.4%) |
| 8742 (2026-10-22) | 3,000 | 11 (0.4%) |
| 8741 candidate + `POOL=1` | 3,000 | 8 |
| 8741 candidate, `index4` patched, `POOL=1` | 5,000 | 0 |
| 8741 candidate, `index4` patched, `POOL=0` | 5,000 | 7 |
| 8741 candidate, `index4` + `g_Scoring` + `wysokosc_koncowki` + `arr52` patched | 5,000 | 0 |

So four shared fields remained after Edward moved `MY_BILANS` into the class:
- `index4` races while bidding. `prepare_selected_positions` rebuilds it on every `get_bid`, and `get_punkty_przebitkowe` reads it.
- `g_Scoring`, `wysokosc_koncowki` and `arr52` are reset in `Class_Initialize`, so creating a new `EPBot` disturbs threads that are mid-auction.

Reusing instances, as in the `POOL=1` runs, is not a workaround, because `index4` races during bidding itself. Until a fixed build ships, the only safe setting is `MAX_CONCURRENCY=1`.
