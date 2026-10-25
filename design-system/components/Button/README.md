# Button

The pill that starts or confirms something. **Primary** (`zec` fill, `on-zec` text, `shadow-press`) appears at most once per view, for the step the screen is for: "Start private swap", "Claim your USDC". **Secondary** is outlined, for the other choice on the screen ("Get a new quote"). **Quiet** is an underlined text button for help and side links ("What is public?").

- Provide: `children` (the label, verb first, sentence case), `onClick`, optional `disabled` and `variant`.
- Disable the primary button while a step cannot be taken yet, and say why next to it; never hide it.
- Never put an amount or a hash inside a button.
