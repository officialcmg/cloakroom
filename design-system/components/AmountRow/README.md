# AmountRow

A label on the left and an exact amount with its unit on the right, in `amount` type with tabular numerals.

- Provide: `label`, `value` (a pre-formatted string such as "299.00"), `unit` ("ZEC", "USDC"), optional `size: 'small'` for fees and secondary figures.
- Format the value before passing it: fixed decimals per asset (ZEC 2 to 4, USDC 2). Never round a fee to zero.
