"""Compile project/tokens.json into tokens.css (the layout the design-system page uses)."""
import json, re, sys

src, out, title = sys.argv[1], sys.argv[2], sys.argv[3]
t = json.load(open(src))
themes = [th["id"] for th in t["color"]["themes"]]
first = themes[0]


def val(v):
    m = re.fullmatch(r"\{([A-Za-z0-9_.-]+)\}", v)
    return f"var(--{m.group(1)})" if m else v


def per_theme(tokens):
    rows = {th: [] for th in themes}
    for tok in tokens:
        v = tok["value"]
        if isinstance(v, str):
            rows[first].append((tok["name"], val(v)))
            if v.startswith("{"):
                for th in themes[1:]:
                    rows[th].append((tok["name"], val(v)))
        else:
            for th in themes:
                if th in v:
                    rows[th].append((tok["name"], val(v[th])))
    return rows


colors = per_theme(t["color"]["tokens"])
shadows = per_theme(t.get("shadow", {}).get("tokens", []))
lines = [f"/* {title} — generated from tokens.json */"]
for i, th in enumerate(themes):
    sel = f':root, [data-theme="{th}"]' if i == 0 else f'[data-theme="{th}"]'
    decl = [f"  --{n}: {v};" for n, v in colors[th] + shadows[th]]
    if i > 0:
        decl.append("  color-scheme: dark;")
    lines += [sel + " {", *decl, "}"]
root = []
for fam in ("spacing", "radius"):
    root += [f"  --{x['name']}: {x['value']};" for x in t.get(fam, {}).get("tokens", [])]
for key, fam in t.items():
    if key in ("name", "version", "color", "type", "spacing", "radius", "shadow") or not isinstance(fam, dict):
        continue
    root += [f"  --{x['name']}: {x['value']};" for x in fam.get("tokens", [])]
root += [f"  --font-{k}: {v};" for k, v in t["type"]["families"].items()]
lines += [":root {", *root, "}"]
for g in t["type"]["groups"]:
    for s in g["styles"]:
        fam = s.get("family", g["family"])
        props = [f"font-family: var(--font-{fam})", f"font-size: {s['fontSize']}", f"line-height: {s['lineHeight']}", f"font-weight: {s['fontWeight']}"]
        if "letterSpacing" in s:
            props.append(f"letter-spacing: {s['letterSpacing']}")
        lines.append(f".{s['name']} {{ {'; '.join(props)}; }}")
open(out, "w").write("\n".join(lines) + "\n")
print("wrote", out)
