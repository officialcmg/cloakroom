# SwapRow

One live swap in the maker's dashboard: its id, the pair, the step it is on in plain words, the next deadline, and its status.

- Provide: `id` (the request id), `sell`, `receive` (formatted with units), `step`, `deadline` ("refund final ~18:07"), `tone`, optional `statusLabel`.
- Put rows inside an element with `role="table"`. Sort by the nearest deadline, not by creation time.
