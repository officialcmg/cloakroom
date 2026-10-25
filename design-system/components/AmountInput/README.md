# AmountInput

The field where the taker types how much to sell, in `amount` type so the figure being entered reads exactly like the figures shown.

- Provide: `label`, `value`, `unit`, `onChange`, and either a `hint` (such as the shielded balance) or an `error`. Give each input a stable `id`.
- The input accepts decimals only (`inputMode="decimal"`); validate on change and say what is wrong and how to fix it: "More than your shielded balance of 6.29 ZEC."
- Never prefill an amount the person did not choose.
