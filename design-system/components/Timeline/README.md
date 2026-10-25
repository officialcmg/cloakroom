# Timeline

The swap's steps from request to claim, one line each, so a person always knows where the swap is and what comes next.

- Provide: `steps`, each `{label, detail?, time?, state}` with state `done`, `now`, `todo` or `failed`. Exactly one step is `now` while the swap runs.
- Labels say what happened or what is happening, from the taker's side: "Maker locked 300 USDC", "Waiting for the lock to be final". `detail` says what to expect: "About 25 minutes on Base. You can close the app."
- On a refund, replace the remaining steps with the refund branch ("Maker refunded", "Sweeping your ZEC back") rather than marking them failed.
