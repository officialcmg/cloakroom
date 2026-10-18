#!/usr/bin/env python3
"""Mutation testing of the Halmos proofs (non-vacuity check).

Copies the project into a scratch directory, applies ONE mutation at a time to the stubbed
escrow copy (test/generated/CloakroomEscrowStubbed.sol -- never src/), runs the Halmos checks
that should catch it, and reports whether each mutant was KILLED (some check failed) or
SURVIVED. Usage:  .venv/bin/python audit/halmos_mutants.py [scratch_dir [M01 M02 ...]]
"""
import os, re, shutil, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HALMOS = os.path.join(ROOT, ".venv", "bin", "halmos")
STUB = "test/generated/CloakroomEscrowStubbed.sol"

# (name, old, new, halmos contract, function regex)
MUTANTS = [
    ("M01 setReady: drop t0 deadline", "if (block.timestamp >= s.t0) revert TooLate();", "",
     "EscrowSMHalmos", "check_setReady_iff"),
    ("M02 setReady: drop maker check", "if (msg.sender != s.maker) revert NotMaker();", "",
     "EscrowSMHalmos", "check_setReady_iff"),
    ("M03 commitClaim: punish window from t1 instead of t2", "|| block.timestamp >= s.t2)", "|| block.timestamp >= s.t1)",
     "EscrowSMHalmos", "check_commitClaim_(iff|fromOpen)"),
    ("M04 commitClaim: accept signer == 0", "if (signer == address(0) || signer != s.payout)", "if (signer != s.payout)",
     "EscrowSMHalmos", "check_commitClaim_iff"),
    ("M05 commitClaim: allow from Locked", "if (st != State.Ready && st != State.Open) revert BadState();",
     "if (st != State.Ready && st != State.Open && st != State.Locked) revert BadState();",
     "EscrowSMHalmos", "check_(commitClaim_iff|anyCall)"),
    ("M06 claim: reveal deadline inclusive", "if (block.timestamp >= uint256(s.lockedAt) + s.revealWindow) revert TooLate();",
     "if (block.timestamp > uint256(s.lockedAt) + s.revealWindow) revert TooLate();",
     "EscrowSMHalmos", "check_claim_iff"),
    ("M07 claim: skip commitment check", "if (commitmentOf(id, a) != s.commitment) revert BadSecret();", "",
     "EscrowSMHalmos", "check_claim_iff"),
    ("M08 claim: pay full amount to payout (fee double-credited)", "+= s.amount - s.relayerFee;", "+= s.amount;",
     "EscrowSMHalmos", "check_claim_iff"),
    ("M09 refund: Open/Ready window not closed at t2", "&& block.timestamp >= s.t1 && block.timestamp < s.t2)", "&& block.timestamp >= s.t1)",
     "EscrowSMHalmos", "check_refund_(iff|OpenReady)"),
    ("M10 refund: Locked refund before window end", "(st == State.Locked && block.timestamp >= uint256(s.lockedAt) + s.revealWindow)",
     "(st == State.Locked && block.timestamp + 1 >= uint256(s.lockedAt) + s.revealWindow)",
     "EscrowSMHalmos", "check_(refund_iff|windowsDisjoint)"),
    ("M11 refund: Claimed can be refunded", "bool ok = (st == State.Open && block.timestamp < s.t0)",
     "bool ok = st == State.Claimed || (st == State.Open && block.timestamp < s.t0)",
     "EscrowSMHalmos", "check_(refund_iff|anyCall)"),
    ("M12 withdraw: credit not zeroed", "credit[token][owner] = 0;", "",
     "EscrowSMHalmos", "check_withdraw_exact"),
    ("M13 withdraw: zeroed after transfer (CEI violation)",
     "        credit[token][owner] = 0;\n        if (amt != 0 && !IERC20Esc(token).transfer(owner, amt)) revert TransferFailed();",
     "        if (amt != 0 && !IERC20Esc(token).transfer(owner, amt)) revert TransferFailed();\n        credit[token][owner] = 0;",
     "EscrowReentryHalmos", "check_withdraw_CEI"),
    ("M14 open: no id-reuse check", "if (swaps[id].state != State.None) revert BadState();", "",
     "EscrowSMHalmos", "check_open_iff"),
    ("M15 open: id not maker-scoped", "id = swapId(msg.sender, salt);", "id = salt;",
     "EscrowSMHalmos", "check_open_(iff|cannotTouch)"),
    ("M16 open: fee <= amount not enforced", "relayerFee > amount || ", "",
     "EscrowSMHalmos", "check_open_iff"),
    ("M17 open: t2 > t1 not enforced", "|| t2 <= t1", "",
     "EscrowSMHalmos", "check_open_iff"),
    ("M18 open: pull tokens before writing state (CEI violation)",
     "        Swap storage s = swaps[id];\n        s.maker = msg.sender;",
     "        if (!IERC20Esc(token).transferFrom(msg.sender, address(this), amount)) revert TransferFailed();\n        Swap storage s = swaps[id];\n        s.maker = msg.sender;",
     "EscrowReentryHalmos", "check_open_CEI"),
    ("M19 claim: makes an external token call", "        s.state = State.Claimed;",
     "        s.state = State.Claimed;\n        IERC20Esc(s.token).transfer(s.payout, 0);",
     "EscrowSMHalmos", "check_claim_iff"),
    ("M20 open: balance-delta check removed (L-01 fix reverted)",
     "        if (IERC20Esc(token).balanceOf(address(this)) - before != amount) revert UnsupportedToken();\n", "",
     "EscrowSMHalmos", "check_open_iff"),
    ("M21 Pallas: canonical-coordinate guard removed (I-01 fix reverted)",
     "        if (x >= P || y >= P) return false;\n        (uint256 X, uint256 Y, uint256 Z) = mulG(k);",
     "        (uint256 X, uint256 Y, uint256 Z) = mulG(k);",
     "PallasHalmos", "check_dlog_rejectsNonCanonical", "src/Pallas.sol", "test_nonCanonicalCoordinatesRejected"),
    ("M22 Pallas: k >= Q guard removed",
     "if (k == 0 || k >= Q) return false;\n        // Audit I-01", "if (k == 0) return false;\n        // Audit I-01",
     "PallasHalmos", "check_dlog_rejectsZeroAndOutOfRange", "src/Pallas.sol", "test_(scalarAliasesRejected|edgeScalarsRejected)"),
]


def run(cmd, cwd, env):
    p = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, timeout=1800)
    return p.returncode, p.stdout + p.stderr


def main():
    scratch = sys.argv[1] if len(sys.argv) > 1 else tempfile.mkdtemp(prefix="halmos-mut-")
    work = os.path.join(scratch, "proj")
    if os.path.exists(work):
        shutil.rmtree(work)
    os.makedirs(work)
    for d in ("src", "test", "audit"):
        shutil.copytree(os.path.join(ROOT, d), os.path.join(work, d), ignore=shutil.ignore_patterns("logs"))
    shutil.copy(os.path.join(ROOT, "foundry.toml"), work)
    os.symlink(os.path.join(ROOT, "lib"), os.path.join(work, "lib"))
    # the scratch copy of src/ is regenerated into test/generated by gen_harnesses.sh only for
    # stub mutants; Pallas mutants (target src/Pallas.sol in the COPY) are read directly by PallasHalmos
    env = dict(os.environ)
    env["PATH"] = os.path.join(ROOT, ".venv", "bin") + os.pathsep + env["PATH"]
    results = []
    only = sys.argv[2:]  # optional mutant ids, e.g. M13 M18
    for m in MUTANTS:
        name, old, new, contract, fn = m[:5]
        target = m[5] if len(m) > 5 else STUB  # src/ paths are mutated ONLY inside the scratch copy
        if only and name.split()[0] not in only:
            continue
        stub_path = os.path.join(work, target)
        original = open(stub_path).read()
        if original.count(old) != 1:
            results.append((name, "NOT APPLIED (pattern count %d)" % original.count(old)))
            continue
        open(stub_path, "w").write(original.replace(old, new))
        code, out = run([HALMOS, "--contract", contract, "--match-test", re.sub(r"^check_", "", fn), "--solver", "z3", "--solver-threads", "1",
                         "--solver-timeout-assertion", "120s"], work, env)
        failed = re.findall(r"\[(FAIL|TIMEOUT|ERROR)\]\S*\s+(\S+)", out)
        passed = re.findall(r"\[PASS\]\S*\s+(\S+)", out)
        compiled = "Compiler run successful" in out or "compilation skipped" in out
        if not compiled:
            verdict = "COMPILE ERROR"
        elif any(f[0] == "FAIL" for f in failed):
            verdict = "KILLED by " + ", ".join(f[1].split("(")[0] for f in failed if f[0] == "FAIL")
        elif failed:
            verdict = "INCONCLUSIVE (" + ", ".join(f[0] for f in failed) + ")"
        else:
            verdict = "SURVIVED (passed: %s)" % ", ".join(p.split("(")[0] for p in passed)
        # Curve arithmetic is beyond symbolic execution, so the Halmos check on a Pallas guard
        # can pass vacuously. Such mutants also run concrete regression tests (m[6]).
        forge_re = m[6] if len(m) > 6 else None
        if forge_re and not verdict.startswith("KILLED"):
            fcode, fout = run(["forge", "test", "-j", "1", "--match-path", "test/CloakroomEscrow.t.sol",
                               "--match-test", forge_re], work, env)
            ffail = re.findall(r"\[FAIL[^\]]*\]\s+(\w+)", fout)
            if ffail:
                verdict = "KILLED by forge " + ", ".join(dict.fromkeys(ffail)) + " (Halmos: " + verdict + ")"
            elif "[PASS]" in fout:
                verdict = "SURVIVED forge " + forge_re + " (Halmos: " + verdict + ")"
            else:
                verdict = "FORGE ERROR (Halmos: " + verdict + ")"
        results.append((name, verdict))
        print(f"{name}: {verdict}", flush=True)
        if not passed and not failed:
            print(out[-3000:], flush=True)
        open(stub_path, "w").write(original)
    print("\n| Mutant | Result |\n|---|---|")
    for n, v in results:
        print(f"| {n} | {v} |")


if __name__ == "__main__":
    main()
