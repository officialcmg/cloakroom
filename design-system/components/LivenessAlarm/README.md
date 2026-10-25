# LivenessAlarm

The maker's warning that it must be online for one swap's refund window, the protocol's L_M rule. Missing the whole window can lose that swap's USDC, so this is the loudest thing in the dashboard.

- Provide: `state` (`upcoming`, `open` or `done`), `window` (local times), `countdown`, `swapId`, and one or two sentences as `children` saying what to keep doing.
- `open` is announced to screen readers and uses `danger`. Show it above everything else until the window closes.
- Pair it with a system notification in the desktop app; the alarm alone is not enough if the window is hidden.
