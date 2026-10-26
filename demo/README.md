# Hosted demo

One container that serves:

- `/`: six slides on the problem, the protocol, what is public, safety, and what in the demo is real;
- `/demo/`: a shared sandbox with the taker and maker apps side by side. A visitor starts a swap from
  the taker app; everyone on the page watches it, with a column explaining what each step made public
  on each chain. While the sandbox starts (about a minute), the page plays a recording of a real swap
  (`web/demo/recording.json`).

Inside, `host.py` runs what `app/dev.py` runs: Zebra 7 regtest with Ironwood rules, anvil with the
escrow and a mock USDC, a house maker and one shared taker, each behind its own `cloakroom serve` on
localhost. The apps in the page are the unchanged `app/ui`; `web/demo/shim.js` routes their API calls.

## What a visitor can do

The public server proxies only these to the two local services:

| Method and path | What |
|---|---|
| `GET /demo/api/{maker,taker}/{status,swaps,log}` | Reads, cached for one second. Keys and shares never appear (the services strip them). |
| `POST /demo/api/taker/swaps` `{"lots": 1}` | Start a one-lot swap, if none is running and the 20 s cooldown has passed; six per address per hour. |
| `GET /demo/api/sandbox` | Idle, running or cooldown, and the last swap's result. |

Everything else under `/demo/api/` is a 404. The maker cannot be stopped from outside, and no request
reaches a key. The keys are generated when the container starts and hold test coins only.

`host.py` refills the taker below 2 ZEC, restarts the maker if it stops quoting, and exits (so the
platform starts a fresh container) if a swap runs for more than 20 minutes, and every 6 hours when idle.

## Run it

```sh
docker build -f demo/Dockerfile -t cloakroom-demo .
docker run --rm -p 8080:8080 cloakroom-demo      # http://localhost:8080
```

`railway.json` at the repository root deploys the same image on Railway.

## Not real

A local anvil chain stands in for Base and USDC is a mock token; finality is about a minute rather than
Base's 25; cover delays are off; one taker wallet is shared by every visitor; the maker is a bot.
