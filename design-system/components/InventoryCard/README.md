# InventoryCard

The maker's balances, one figure per row: what can be quoted now, what is locked in open swaps, and the gas left.

- Provide: `title`, optional `status` (a `StatusChip`, such as Quoting or Paused), `items` with `{label, value, unit, note?}`.
- Put locked amounts in the note under the free balance, not as a separate total.
