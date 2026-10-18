"""Print selected RPC responses from evidence/run.log, truncating long hex fields.

usage: python3 -I scripts/excerpt.py evidence/run.log
"""
import json, re, sys

text = open(sys.argv[1]).read()
blocks = re.split(r"\n\$ rpc ", text)
want = {"generatetoaddress", "getblock", "getrawtransaction", "sendrawtransaction", "getrawmempool", "z_gettreestate", "generate"}

def trunc(v):
    if isinstance(v, dict):
        return {k: trunc(x) for k, x in v.items() if k not in ("valuePools",)}
    if isinstance(v, list):
        return [trunc(x) for x in v]
    if isinstance(v, str) and len(v) > 130:
        return v[:48] + f"...[{len(v)} chars, full value in evidence/run.log]"
    return v

for b in blocks[1:]:
    head, _, rest = b.partition("\n")
    method = head.split(" ", 1)[0]
    if method not in want:
        continue
    # skip the per-block scan calls (verbosity 0 tx fetches, height-indexed getblock)
    if method == "getrawtransaction" and head.rstrip().endswith(", 0]"):
        continue
    if method == "getblock" and re.match(r'getblock \["\d+",', head):
        continue
    end = rest.find("\n}") + 2
    resp = json.loads(rest[:end])
    print(f"$ rpc {head}")
    print(json.dumps(trunc(resp), indent=2))
    print()
