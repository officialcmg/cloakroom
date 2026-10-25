# QuoteForm

The taker's first screen: how much to sell, roughly what comes back, which maker, how private, and the button that asks for a quote.

- Provide: `sell` and `onSellChange`, `balance`, `receive` (the estimate, net of fees), `maker` (name plus "signed directory" once its signature checks), `privacy` and `onPrivacyChange`, optional `privacyOptions`, `privacyNote`, `action`, `onSubmit`.
- Default privacy to Private. Show both options with their real time estimates; never hide the slow one.
- After the quote arrives, replace this form with a `SwapCard` showing the quoted terms and "Start private swap".
