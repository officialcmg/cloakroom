# Cloakroom design system

Direction "Shielded Yellow": white paper, black 2px outlines, Zcash yellow for the one action that matters, in light and dark themes. The brand book, with the rules for colour, type, copy and status, is in [`BRAND.md`](BRAND.md).

| Path | What |
|---|---|
| `tokens.json` | Source of truth for every colour, type style, spacing step, radius, stroke and shadow. |
| `tokens.css` | The tokens compiled to CSS custom properties. Set `data-theme="light"` or `"dark"` on `<html>`. Regenerate with `python3 tools/tokens_css.py tokens.json tokens.css Cloakroom`. |
| `components/bundle.js`, `bundle.css`, `index.d.ts` | 14 React 18 components as `window.Cloakroom`, their styles, and their props. |
| `components/<Name>/README.md`, `preview.html` | Usage rules and a working example for each component. |
| `assets/Logos`, `assets/Icons` | The claim-ticket mark and nine 24px icons as SVG. |
| `fonts.css`, `fonts/` | Archivo and IBM Plex Mono as local woff2 files. |
| `gallery.html` | Every component in both themes. Open it in a browser. |

Load order in a page: `fonts.css`, `tokens.css`, `components/bundle.css`, React and ReactDOM 18, then `components/bundle.js`. Fonts are local files in `fonts/` (SIL OFL), so a page built on this system makes no requests to Google.

The same system is published as a browsable page in Claude: https://claude.ai/artifact/BYLf1Ht3h7GZwshQasU1i1 (private until shared). Change `tokens.json` here and regenerate `tokens.css`; keep the two copies in step.
