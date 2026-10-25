# SwapCard

The one card that holds money: what you sell, what you get, the quote's terms and the action. It is the only element with `shadow-hard`.

- Provide: `sell` and `receive` (`{label, value, unit}`), `meta` (quote, premium and relayer fee in one `caption` line), `action` and `onAction`, optional `status` (a `StatusChip`) and `disabled`.
- One per screen. Below it, show the swap's `Timeline` once the swap has started.
- The receive amount is net of the relayer fee, and `meta` states the fee.
