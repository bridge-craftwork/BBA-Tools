# BBA Server API contract

What `bba-server` expects from a client, and what it returns. Written for the
people building against it — the BBOAlert and PBSforBBO extensions,
Bridge-Classroom, ClubGameAnalysis, and anything added later.

**Base URL:** `https://bba.harmonicsystems.com`

This describes the service **as it behaves today** (verified against a running
server on 2026-09-10, EPBot 8740). Where the behavior is surprising it is
written down as-is and called out under [Known hazards](#known-hazards) rather
than quietly idealized. There is no API version header and no `/v1` prefix:
changes are additive in practice, and the response omits fields rather than
sending nulls, so a client should ignore unknown fields and tolerate missing
optional ones.

## Conventions

- Request and response bodies are JSON, UTF-8.
- Response fields are **camelCase**. Request fields are camelCase too, but
  `snake_case` is also accepted for the multi-word ones (`single_dummy`,
  `auction_prefix`, `include_all_meanings`, `board_number`) — they are serde
  aliases, so either spelling works and they mean the same thing.
- **Optional response fields are omitted entirely when unset**, never sent as
  `null`. Test with `'auction' in response`, not `response.auction !== null`.
- Application-level failures return **HTTP 200** with `success: false`. Only
  malformed transport gets a 4xx — see [Errors](#errors).

## Headers

| Header | Required | Purpose |
|---|---|---|
| `Content-Type: application/json` | **Yes** on POST | Omitting it is a hard `415`, before any handler runs. |
| `X-Client-Info` | No, but please | Identifies your client in the audit log and admin dashboard. |
| `X-Client-Version` | No | Free-form; logged to the `ClientVersion` column. |
| `X-API-Key` | Only if the server has `API_KEY` set | Production currently runs with it **empty**, so no key is required. If it is ever set, send the header (or `?apiKey=` in the query string) or get a `401`. |

### `X-Client-Info` format

```
X-Client-Info: ext=PBSforBBO; browser=Chrome; os=Windows
```

Parsed by `get_client_info()` in [`bba-server/src/routes/api.rs`](../bba-server/src/routes/api.rs).
Semicolon-separated `key=value` pairs; keys are `ext`, `browser`, `os`;
whitespace around keys and values is trimmed; **unknown keys are ignored and
every key is optional**, so `ext=BridgeClassroom` on its own is valid and is
already a large improvement over sending nothing.

Send it. Requests without it are logged with all three columns empty and show
up in the dashboard as an unattributed "(unknown)" band — currently 87% of all
auction traffic. Current known values of `ext`: `BBOAlert`, `PBSforBBO`.

### CORS

Browser clients must call from an allow-listed origin (exact match): the
`bridgebase.com` and `bridge-classroom.com` / `.org` families including
`game-analysis.`, plus `localhost`/`127.0.0.1` on ports 3000-3001, 5173-5199
(Vite dev) and 4173-4199 (Vite preview). Allowed request headers are
`content-type`, `x-client-version`, `x-client-info`, `x-api-key` — so adding
client-info to a browser client needs no server change. A rejected origin
fails fast (~20ms) and looks to the user like the service stalling.

Server-to-server callers are unaffected by CORS.

---

## `POST /api/auction/generate`

Bid a deal and return the auction. This is the endpoint that matters.

### Request

| Field | Type | Required | Default / notes |
|---|---|---|---|
| `deal.pbn` | string | **yes** | PBN deal string, e.g. `N:.63.AKQ987.A9732 A8654.KQ5.T.QJT4 …`. Must include the `N:` prefix. |
| `deal.dealer` | string | **yes** | `N` \| `E` \| `S` \| `W`, or the full word `NORTH`/`EAST`/`SOUTH`/`WEST`. Case-insensitive. **Anything else silently becomes North** — see hazards. |
| `deal.vulnerability` | string | **yes** | `None` \| `NS` \| `EW` \| `Both`. Also accepts `All`, the spelled-out `NORTHSOUTH`/`EASTWEST`, and hyphens (`N-S`). **Anything else silently becomes None.** |
| `deal.scoring` | string | no | `MP` (default). `IMP` selects IMP scoring; any other value is treated as MP. |
| `scenario` | string | no | PBS scenario name; the server looks up which convention cards that scenario implies. An unrecognized name falls back to the defaults **without an error**. |
| `conventions.ns` / `conventions.ew` | string | no | Convention card names (`.bbsa` basenames). Defaults `21GF-DEFAULT` / `21GF-GIB`. **`conventions` wins over `scenario`** when both are sent. |
| `auctionPrefix` | string[] | no | Forced opening calls the engine must continue from. Each entry is `Pass`, `X`, `XX`, or `{1-7}{C\|D\|H\|S\|NT}`. Invalid entries **do** fail the request (`success: false`). |
| `singleDummy` | bool | no | `false`. When true the response also carries `contract`, `declarer`, `result`, `score`, `boardHash`. Costs latency. |
| `includeAllMeanings` | bool | no | `false`. When true, `meanings[]` carries `meaning`/`meaningExtended` for every bid the engine explains, not only alertable ones. Roughly triples response size. |
| `boardNumber` | number | no | `1`. Only used to derive the `boardHash` nibble when `singleDummy` is true. |

```bash
curl -X POST https://bba.harmonicsystems.com/api/auction/generate \
  -H 'Content-Type: application/json' \
  -H 'X-Client-Info: ext=BridgeClassroom; browser=Chrome; os=macOS' \
  -d '{
        "deal": {
          "pbn": "N:.63.AKQ987.A9732 A8654.KQ5.T.QJT4 KQT9.J98742.J.K8 J732.AT.65432.65",
          "dealer": "N",
          "vulnerability": "None"
        }
      }'
```

### Response

Always HTTP 200 (barring the transport rejections below).

| Field | Type | When present |
|---|---|---|
| `success` | bool | always |
| `auction` | string[] | on success — e.g. `["1D","1S","2H","3S","Pass","Pass","Pass"]`. Calls are `Pass`, `X`, `XX`, `1C`…`7NT`. |
| `auctionEncoded` | string | on success — BBO-style compact form (`Pass`→`--`, `X`→`Db`). |
| `conventionsUsed` | `{ns, ew}` | **always, including on failure** — tells you which cards were actually resolved. |
| `meanings` | BidMeaning[] | on success — one entry per call, `{position, bid, meaning?, meaningExtended?, isAlert}`. `meaning`/`meaningExtended` are omitted for calls the engine did not explain; by default only alertable calls carry them. |
| `contract` | string | `singleDummy: true` and the auction produced a contract — e.g. `3SX`. |
| `declarer` | string | as `contract` — `N`/`E`/`S`/`W`. |
| `result` | number | `singleDummy: true` — single-dummy estimated tricks. |
| `score` | number | `singleDummy: true` — score from the **NS** perspective (negative means NS is minus). |
| `boardHash` | string | `singleDummy: true` — 28-hex BBA-style board fingerprint. |
| `error` | string | `success: false` only — human-readable, not a stable code. |

Real response to the curl above (`meanings` truncated — it carries one entry
per call):

```json
{
  "success": true,
  "auction": ["1D", "1S", "2H", "3S", "Pass", "Pass", "X", "Pass", "Pass", "Pass"],
  "auctionEncoded": "1D1S2H3S----Db------",
  "conventionsUsed": { "ns": "21GF-DEFAULT", "ew": "21GF-GIB" },
  "meanings": [
    { "position": 0, "bid": "1D", "isAlert": false },
    { "position": 2, "bid": "2H",
      "meaning": "1X-(Y)-2Z forcing",
      "meaningExtended": " 2H ALERT. 8 to 21 total points, 0 to 13 cards in clubs, 0 to 13 cards in diamonds, 5 to 13 cards in hearts, 0 to 6 cards in spades.",
      "isAlert": true },
    { "position": 3, "bid": "3S", "isAlert": false }
  ]
}
```

Note `auctionEncoded` is fixed-width **two characters per call** (`--` = Pass,
`Db` = X, `Rd` = XX), so a 10-call auction is 20 characters. `meaningExtended`
has a leading space and repeats the alert marker — pass it through, don't
assume it is trimmed.

Failure keeps the same envelope:

```json
{
  "success": false,
  "conventionsUsed": { "ns": "21GF-DEFAULT", "ew": "21GF-GIB" },
  "error": "Invalid PBN deal: Missing colon in PBN deal"
}
```

Real `error` values seen: `Invalid PBN deal: …`, `Convention card not found on
GitHub: NOPE-CARD (HTTP 404 …)`, `EPBot FFI error (code 0): Invalid
auctionPrefix at index 0: …`. Treat the string as a message for a human, not a
branchable code.

### Latency and concurrency

Auctions are generated on a blocking thread pool behind a semaphore
(`MAX_CONCURRENCY`, currently 4). A typical auction is tens to a few hundred
ms; the first call for an unseen convention card also fetches it from GitHub.
Clients should set a generous timeout (10s+) and avoid firing large batches in
parallel — they will queue anyway.

---

## `POST /api/scenario/select`

Records that a user picked a scenario. Analytics only; it drives the dashboard's
Scenario History and the scenario-selection half of the usage charts.

Request: `{"scenario": "Fourth_Suit_Forcing"}` — the field is optional and a
missing one is logged as empty. Response: `{"success": true}`, always.

Send `X-Client-Info` here too; extension clients do, which is why every row in
the scenario log has client info while most auction rows do not.

## `GET /api/scenarios`

Lists available PBS scenario names, sourced from the
Practice-Bidding-Scenarios repo on GitHub.

```json
{ "scenarios": ["1C_WalshStyle", "1M-3N_Picture_Bid", "1N", "..."] }
```

347 entries at time of writing. On upstream failure it returns HTTP 200 with
`{"scenarios": [], "error": "..."}` — an empty list, not an error status.

## `GET /health`

`{"status":"healthy","timestamp":"2026-09-10T20:54:56.266192+00:00"}`

**A 200 here does not mean the engine works.** It reports process liveness
only; it does not exercise EPBot. Verify a deploy with a real
`POST /api/auction/generate`.

## Admin endpoints

`/admin/*` is an internal dashboard (usage stats, audit logs), gated by
`ADMIN_KEY` or an allow-listed anonymized IP. Not part of the client contract;
shape may change without notice.

---

## Errors

Two distinct layers, and clients need to handle both:

**Transport rejections** — plain-text body, *not* the JSON envelope:

| Status | Cause | Body |
|---|---|---|
| `400` | Body is not valid JSON | `Failed to parse the request body as JSON: …` |
| `415` | Missing/incorrect `Content-Type` | `Expected request with 'Content-Type: application/json'` |
| `422` | Valid JSON, wrong shape (missing `deal`, wrong types) | `Failed to deserialize the JSON body into the target type: …` |
| `401` | `API_KEY` configured and the key is missing/wrong | JSON error object |
| `404` | Unknown route | empty |

**Application failures** — HTTP `200`, `success: false`, `error` set. Bad PBN,
missing convention card, engine failure, invalid `auctionPrefix`.

So: check `response.ok` **and** `body.success`. Checking only the status code
silently treats every engine failure as success — parsing `auction` on a
failed response yields `undefined`, not an exception.

## Known hazards

Documented, deliberately not changed — existing clients depend on the loose
parsing. Client authors should know about them.

1. **Unknown `dealer` and `vulnerability` values are silently coerced, not
   rejected.** `parse_dealer()` falls through to North and
   `parse_vulnerability()` to None. This is not theoretical — the same deal
   with `"dealer": "S"` versus `"dealer": "South "` (trailing space) returns two
   completely different auctions:

   ```
   "S"       → 1H Pass 2D Pass 2H Pass …
   "South "  → 1D 1S 2H 3S Pass Pass …   (bid as if North dealt)
   ```

   Both come back `success: true`. Validate seat and vulnerability strings on
   the client before sending.

   **This has already happened in production.** Nine requests on 2026-08-27
   arrived with `dealer` values of `G` and `K` — one user, BBOAlert 1.9.26,
   every request invalid — and all nine were bid as if North dealt and
   returned `success: true`. Across 2,729 auction rows those 9 are the only
   invalid dealers, and there are no invalid vulnerability values at all.

2. **An unrecognized `scenario` silently falls back to the default cards.** You
   get a perfectly good auction bid with `21GF-DEFAULT`/`21GF-GIB` and no
   indication the scenario was not found. Compare `conventionsUsed` against
   what you expected if this matters.

3. **Engine and convention-fetch failures are HTTP 200.** See above.

4. **`/health` does not test the engine.** See above.

5. **The API-key middleware skips only `/health` and `/admin`**, despite a code
   comment claiming scenarios are exempt too. Moot while `API_KEY` is empty in
   production, but if a key is ever set, `/api/scenarios` will start requiring
   it and any client listing scenarios unauthenticated breaks.

## Checklist for a new client

- [ ] Send `Content-Type: application/json`
- [ ] Send `X-Client-Info: ext=<YourClient>; browser=<b>; os=<o>` (and
      `X-Client-Version` if you have one)
- [ ] Validate `dealer` / `vulnerability` before sending — the server won't
- [ ] Check `response.ok` **and** `body.success`
- [ ] Treat `error` as a message, not a code
- [ ] Tolerate omitted optional fields and ignore unknown ones
- [ ] Timeout ≥ 10s; don't fan out large parallel batches
- [ ] Browser client? Confirm your origin is in the CORS allow-list
- [ ] Call `POST /api/scenario/select` when a user picks a scenario, so usage
      analytics see it
