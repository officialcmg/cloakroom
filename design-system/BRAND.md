Cloakroom lets a taker swap shielded ZEC for USDC on Base with a market maker, with no custodian and no bridge. The interface has one job: make a slow, cryptographic process feel safe and obvious. It is bright, outlined and plain-spoken, and it always says what is public and what is not.

## Content fundamentals

- Write to the taker as "you"; the other side is "the maker". Never "user", "counterparty" or "LP".
- Sentence case everywhere, including buttons: "Start private swap", "Claim your USDC".
- Numbers are exact and carry their unit: "299.00 USDC", "0.0005 ZEC", "~25 min". Never round a fee away.
- Say where a swap is and what happens next, in one line each: "Waiting for the lock to be final · ~25 min".
- Reassure with facts, not adjectives: "Your ZEC is safe at every step. If the maker disappears, you get it back after the deadline." Never "secure", "trustless" or "military-grade".
- Name what leaks. Any screen that shows a public address or an amount on Base says so: "Visible on Base: amount, payout address."
- No emoji, no exclamation marks.

## Visual foundations

**Colour.** `paper` ground, `ink` text and outlines, and `zec` yellow for exactly one primary action per view (with `on-zec` text on it). Yellow is a fill, never text: as a text accent use `zec-text`. Tints (`zec-soft`) mark held or waiting things, such as the shielded-balance pill and the Locked chip. Status colours each mean one outcome and always come with a word: `final` (done, settled), `refund` (funds coming back, a safe outcome, so blue and never red), `danger` (you must act). Each status pairs its text colour with its `-soft` fill.

**Outlines, not shadows.** Cards, chips, inputs and the secondary button take a `stroke` outline in `line`. The only shadow is `shadow-hard`, on the swap card, because it is the object that holds money; the primary button carries `shadow-press` and loses it when pressed. Use `line-soft` hairlines only for dividers inside a card.

**Shape.** Actions and chips are pills (`radius-pill`). Cards and notices use `radius-md`, inputs and hash chips `radius-sm`. Nothing else is rounded.

**Type.** Archivo 800 for amounts (`amount`), titles (`title`) and the page title (`display`); Archivo 400 for `body` and `caption`; uppercase `label` for field labels and chips. Hashes, addresses and block numbers are `hash` in IBM Plex Mono, shortened to head and tail (`0xCCE4…4283`) with the full value on hover. Amounts always use tabular numerals.

**Space and layout.** The taker app is one column, at most 440px wide, centred, with `space-6` between cards and `space-4` inside them. The maker dashboard is a grid of the same cards. Spacing comes only from the `space-*` steps.

**State and motion.** The current timeline step is the only thing that moves: a slow pulse of its `zec` ring, removed under `prefers-reduced-motion`. Done steps are filled `final` dots, future steps hollow `ink-muted` rings, a failed step a `danger` dot with its reason.

**Focus.** Keyboard focus is a solid `stroke-heavy` ring in `focus` with a 2px offset: ink on light, yellow on dark.

## Iconography

Nine outline icons on a 24px grid at a 2px stroke, the same weight as `stroke`: shield, lock, clock, check, return, alert, copy, external, arrow. In the app use the `Icon` component, which draws them in the text colour. The Icons group holds the same icons as SVG files drawn in `ink` light (#111111), for design tools. An icon sits beside a word and never replaces it: "Locked" with a lock, never a lock alone. No emoji.

## Logo

The mark is a claim ticket: a `zec` pill outlined in `line`, with one punched hole in `ink`. The wordmark is "cloakroom" in lowercase Archivo 800 beside it. Use the `Logo` component in the app and the files in the Logos group elsewhere. On a yellow ground use the mark alone. Never pair it with the Zcash mark.

## The maker dashboard

The maker sees a grid of cards: `InventoryCard` first, then the live swaps as `SwapRow`s sorted by the nearest deadline. Any `LivenessAlarm` in the `open` state sits above everything else until its window closes, because missing that window can lose the swap's USDC.

## Status words

| State | Chip | Use |
|---|---|---|
| Final | `final` on `final-soft` | The claim or refund cannot be undone. |
| Locked | `ink` on `zec-soft` | The escrow holds the USDC. |
| Waiting | `ink` on `zec-soft` | A step is waiting on a chain or a delay. |
| Refunding | `refund` on `refund-soft` | Funds are on their way back. |
| Action needed | `danger` on `danger-soft` | You must do something, such as a maker coming online for its liveness window. |

## Using this system

Components are in `components/bundle.js` as `window.Cloakroom` and need React 18 on the page. Load Archivo (400, 600, 800) and IBM Plex Mono (400) from Google Fonts; `components/bundle.css` imports them. Use the colour tokens through CSS variables (`var(--zec)`), never as literal hex.
