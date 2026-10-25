# Cloakroom app

The local app: a web UI served by `cloakroom serve` on your own computer. It drives the same maker and
taker state machines the harness and the testnet runs use, and reads the same swap files they write.

- **Your keys stay on this machine.** The service listens on `127.0.0.1` only and never returns keys,
  shares or seeds; the API removes them from every swap record.
- **No third-party requests.** Fonts and React are served from this repository, and the page's content
  security policy allows only its own origin.
- **Other websites cannot drive it.** Requests must name localhost in `Host` (no DNS rebinding), and every
  POST must carry an `X-Cloakroom: 1` header, which a page on another site cannot send.

## Try it locally

Needs Docker, Foundry and a built node (`cd node && cargo build`).

```sh
python3 -I app/dev.py
```

This starts Zebra regtest and anvil, creates a funded maker and taker, and opens:

- maker: http://127.0.0.1:7801/ (press **Start quoting**)
- taker: http://127.0.0.1:7802/ (press **Start private swap**)

A swap takes about five minutes on the local chains, most of it waiting for anvil blocks to become final.

## Run it against your own setup

```sh
cloakroom serve --dir <party dir> --port 7800 --ui app/ui --ds design-system
```

`<party dir>` is a maker or taker directory with its `config.json` and keys, the same one `cloakroom maker`
or `cloakroom taker` uses. The role comes from the config: a directory with an `evm_key` is a maker.

## API

| Method and path | What |
|---|---|
| `GET /api/status` | Role, shielded balance, the maker's EVM balances, public parameters, whether the maker or taker process is running. |
| `GET /api/swaps` | Every swap record in `swaps/`, newest first, secrets removed. |
| `GET /api/log` | The last stage lines of the running process's log. |
| `POST /api/swaps` `{"lots": n}` | Taker: start a swap of `n` lots. |
| `POST /api/maker/start` | Maker: start quoting. |
| `POST /api/stop` | Stop the running process. Swaps resume from their files when it starts again. |

## Not in this version

- The taker accepts a quote automatically when it is within the configured limits (`max_price`,
  `max_premium`, `max_horizon_secs`); a review-and-confirm step needs a pause in the taker's state machine.
- One taker swap at a time.
- The privacy setting is read from the config (`cover_*`) rather than chosen per swap.
- No desktop packaging (Tauri) or system notifications for the maker's liveness alarm yet.
