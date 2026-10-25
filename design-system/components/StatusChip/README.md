# StatusChip

A pill naming a swap's state in one word: Final, Locked, Waiting, Refunding or Action needed. Each tone pairs its text colour with its `-soft` fill and always shows the word, so state never depends on colour alone.

- Provide: `tone` (`final`, `locked`, `waiting`, `refund`, `danger`); `children` only to replace the default word.
- One chip per swap, in the swap card's header or a dashboard row.
- `refund` is blue because getting your funds back is a safe outcome. Keep `danger` for things the person must act on.
