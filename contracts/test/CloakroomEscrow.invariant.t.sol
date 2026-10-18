// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

// Foundry stateful invariant tests against the REAL src/CloakroomEscrow.sol and REAL src/Pallas.sol
// (no stubs). A handler drives open/setReady/commitClaim/claim/refund/withdraw/warp across 3 makers,
// a pool of payout keys, relayers and Pallas key shares, with both valid and invalid inputs.

import {Test} from "forge-std/Test.sol";
import {StdInvariant} from "forge-std/StdInvariant.sol";
import {CloakroomEscrow} from "../src/CloakroomEscrow.sol";
import {Pallas} from "../src/Pallas.sol";

contract InvToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    uint256 public totalSupply;

    function mint(address to, uint256 v) external {
        balanceOf[to] += v;
        totalSupply += v;
    }

    function approve(address s, uint256 v) external returns (bool) {
        allowance[msg.sender][s] = v;
        return true;
    }

    function transfer(address to, uint256 v) external returns (bool) {
        balanceOf[msg.sender] -= v;
        balanceOf[to] += v;
        return true;
    }

    function transferFrom(address f, address to, uint256 v) external returns (bool) {
        allowance[f][msg.sender] -= v;
        balanceOf[f] -= v;
        balanceOf[to] += v;
        return true;
    }
}

/// Affine [k]G for test fixtures: real Pallas.mulG, then one modular inversion (modexp precompile).
library PallasAffine {
    function mul(uint256 k) internal view returns (uint256 x, uint256 y) {
        (uint256 X, uint256 Y, uint256 Z) = Pallas.mulG(k);
        uint256 zi = _inv(Z);
        uint256 zi2 = mulmod(zi, zi, Pallas.P);
        x = mulmod(X, zi2, Pallas.P);
        y = mulmod(Y, mulmod(zi2, zi, Pallas.P), Pallas.P);
    }

    function _inv(uint256 a) private view returns (uint256 r) {
        (bool ok, bytes memory out) =
            address(5).staticcall(abi.encodePacked(uint256(32), uint256(32), uint256(32), a, Pallas.P - 2, Pallas.P));
        require(ok);
        r = abi.decode(out, (uint256));
    }
}

contract EscrowHandler is Test {
    CloakroomEscrow public esc;
    InvToken public tok;

    address[3] public makers = [address(0xA001), address(0xA002), address(0xA003)];
    uint256[3] public payoutKeys = [uint256(0xB001), uint256(0xB002), uint256(0xB003)];
    address[2] public relayers = [address(0xC001), address(0xC002)];
    uint256[4] public shares; // Pallas scalars
    uint256[4] public sx;
    uint256[4] public sy;

    struct Ghost {
        bytes32 id;
        uint256 aIdx;
        uint256 bIdx;
        uint256 payoutIdx;
        bool readySeen;
        bool validCommit;
        bool lockedByPunish;
        bool claimed;
        bool refunded;
    }

    Ghost[] public ghosts;
    mapping(bytes32 => bool) public known;

    // ghost accounting
    uint256 public deposited;
    uint256 public withdrawn;
    uint256 public settledCredited;
    bool public violation;
    uint256 public punishLocks;
    string public violationWhy;

    // call statistics
    mapping(bytes32 => uint256) public okCount; // keyed by keccak256(name)
    uint256 public totalTries;

    uint256 constant MINT = 1e36;

    constructor() {
        esc = new CloakroomEscrow();
        tok = new InvToken();
        for (uint256 i; i < 3; i++) {
            tok.mint(makers[i], MINT);
            vm.prank(makers[i]);
            tok.approve(address(esc), type(uint256).max);
        }
        uint256[4] memory ks = [
            uint256(1),
            0x195661e2e4789b7f57137b2e777736d79720270638d12e4882fd95e52a73d6b3,
            0x2b8bbfabc504814096b330ffba3d828335e8db236f79aa7fded4d77640369e2a,
            Pallas.Q - 1
        ];
        for (uint256 i; i < 4; i++) {
            shares[i] = ks[i];
            (sx[i], sy[i]) = PallasAffine.mul(ks[i]);
        }
    }

    function ghostCount() external view returns (uint256) {
        return ghosts.length;
    }

    function ghostAt(uint256 i) external view returns (Ghost memory) {
        return ghosts[i];
    }

    function payoutAddr(uint256 i) public view returns (address) {
        return vm.addr(payoutKeys[i % 3]);
    }

    function _flag(string memory why) internal {
        violation = true;
        violationWhy = why;
    }

    function _state(bytes32 id) internal view returns (CloakroomEscrow.State st) {
        (,,,,,,,,, st,,,,,,,) = esc.swaps(id);
    }

    function _times(bytes32 id)
        internal
        view
        returns (uint64 t0, uint64 t1, uint64 t2, uint64 w, uint64 lockedAt, uint256 amount, uint256 fee)
    {
        (,,,, t0, t1, t2, w, lockedAt,, amount, fee,,,,,) = esc.swaps(id);
    }

    /// Pick a swap, preferring (with probability 3/4) one in state `want` (or `want2`), searching
    /// from a random offset; otherwise a uniformly random swap. Keeps invalid calls in the mix.
    function _pick(uint256 gi, CloakroomEscrow.State want, CloakroomEscrow.State want2) internal view returns (uint256) {
        uint256 n = ghosts.length;
        uint256 start = gi % n;
        if ((gi >> 128) % 4 == 0) return start;
        for (uint256 k; k < n; k++) {
            uint256 i = (start + k) % n;
            CloakroomEscrow.State st = _state(ghosts[i].id);
            if (st == want || st == want2) return i;
        }
        return start;
    }

    // ------------------------------------------------------------------ actions

    function open(
        uint256 makerIdx,
        uint256 salt,
        uint256 amount,
        uint256 fee,
        uint256 d0,
        uint256 d1,
        uint256 d2,
        uint256 w,
        uint256 payoutIdx,
        uint256 aIdx,
        uint256 bIdx,
        uint8 badSel
    ) external {
        totalTries++;
        bool badParams = badSel % 16 == 0;
        address mk = makers[makerIdx % 3];
        salt = salt % 4; // small salt space -> frequent id reuse attempts
        amount = bound(amount, 1, 1e24);
        fee = badParams ? bound(fee, amount + 1, amount + 1e6) : bound(fee, 0, amount);
        uint64 t0 = uint64(block.timestamp + bound(d0, 1, 3 hours));
        uint64 t1 = uint64(t0 + bound(d1, 1, 3 hours));
        uint64 t2 = uint64(t1 + bound(d2, 1, 3 hours));
        uint64 W = uint64(bound(w, 1, 6 hours));
        aIdx %= 4;
        bIdx %= 4;
        bytes32 id = esc.swapId(mk, bytes32(salt));
        bool existed = known[id];
        vm.prank(mk);
        try esc.open(
            bytes32(salt),
            payoutAddr(payoutIdx),
            address(tok),
            amount,
            fee,
            sx[aIdx],
            sy[aIdx],
            sx[bIdx],
            sy[bIdx],
            t0,
            t1,
            t2,
            W
        ) returns (bytes32 rid) {
            okCount[keccak256("open")]++;
            if (badParams) _flag("open accepted fee > amount");
            if (existed) _flag("open overwrote an existing id");
            if (rid != id) _flag("open returned id != swapId(maker, salt)");
            known[id] = true;
            deposited += amount;
            ghosts.push(Ghost(id, aIdx, bIdx, payoutIdx % 3, false, false, false, false, false));
        } catch {
            if (!badParams && !existed) _flag("valid open reverted");
        }
    }

    function setReady(uint256 gi, uint8 asMakerSel, uint256 who) external {
        totalTries++;
        bool asMaker = asMakerSel % 8 != 0; // biased towards valid calls
        if (ghosts.length == 0) return;
        Ghost storage g = ghosts[_pick(gi, CloakroomEscrow.State.Open, CloakroomEscrow.State.Open)];
        (address mk,,,,,,,,,,,,,,,,) = esc.swaps(g.id);
        address caller = asMaker ? mk : address(uint160(bound(who, 1, type(uint160).max)));
        vm.prank(caller);
        try esc.setReady(g.id) {
            okCount[keccak256("setReady")]++;
            if (caller != mk) _flag("setReady by non-maker");
            g.readySeen = true;
        } catch {}
    }

    function commitClaim(uint256 gi, uint8 sigSel, uint8 commitSel, uint256 relIdx, uint256 badKey) external {
        totalTries++;
        bool validSig = sigSel % 8 != 0;
        bool correctCommit = commitSel % 8 != 0;
        if (ghosts.length == 0) return;
        Ghost storage g = ghosts[_pick(gi, CloakroomEscrow.State.Ready, CloakroomEscrow.State.Open)];
        CloakroomEscrow.State pre = _state(g.id);
        address rel = relayers[relIdx % 2];
        bytes32 c = esc.commitmentOf(g.id, correctCommit ? shares[g.aIdx] : shares[(g.aIdx + 1) % 4]);
        uint256 key = validSig ? payoutKeys[g.payoutIdx] : bound(badKey, 1, 1e70);
        if (!validSig && vm.addr(key) == payoutAddr(g.payoutIdx)) key += 1;
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, esc.commitDigest(g.id, c, rel));
        try esc.commitClaim(g.id, c, rel, v, r, s) {
            okCount[keccak256("commitClaim")]++;
            if (!validSig) _flag("commitClaim accepted a non-payout signature");
            (, uint64 t1, uint64 t2,,,,) = _times(g.id);
            bool normal = pre == CloakroomEscrow.State.Ready && block.timestamp < t1;
            bool punish = (pre == CloakroomEscrow.State.Open || pre == CloakroomEscrow.State.Ready)
                && block.timestamp >= t2;
            if (!normal && !punish) _flag("commitClaim outside its windows");
            if (pre == CloakroomEscrow.State.Open && block.timestamp < t2) _flag("commit from Open before t2");
            g.validCommit = correctCommit;
            g.lockedByPunish = !normal;
            if (!normal) punishLocks++;
        } catch {}
    }

    function claim(uint256 gi, uint8 sel, uint256 junk) external {
        totalTries++;
        bool correct = sel % 8 != 0;
        if (ghosts.length == 0) return;
        Ghost storage g = ghosts[_pick(gi, CloakroomEscrow.State.Locked, CloakroomEscrow.State.Locked)];
        CloakroomEscrow.State pre = _state(g.id);
        (,,, uint64 w, uint64 lockedAt,,) = _times(g.id);
        uint256 a = correct ? shares[g.aIdx] : junk;
        try esc.claim(g.id, a) {
            okCount[keccak256("claim")]++;
            if (pre != CloakroomEscrow.State.Locked) _flag("claim from non-Locked");
            if (!g.readySeen && !g.lockedByPunish) _flag("claim without Ready or punish lock");
            if (block.timestamp >= uint256(lockedAt) + w) _flag("claim after reveal window");
            if (a != shares[g.aIdx]) _flag("claim with wrong scalar");
            if (!g.validCommit) _flag("claim succeeded although the commitment was for another scalar");
            if (g.refunded) _flag("claim after refund");
            g.claimed = true;
            (,,,,, uint256 amount,) = _times(g.id);
            settledCredited += amount;
        } catch {}
    }

    function refund(uint256 gi, uint8 sel, uint256 junk) external {
        totalTries++;
        bool correct = sel % 8 != 0;
        if (ghosts.length == 0) return;
        Ghost storage g = ghosts[_pick(gi, CloakroomEscrow.State.Locked, CloakroomEscrow.State.Open)];
        CloakroomEscrow.State pre = _state(g.id);
        (uint64 t0, uint64 t1, uint64 t2, uint64 w, uint64 lockedAt, uint256 amount,) = _times(g.id);
        uint256 b = correct ? shares[g.bIdx] : junk;
        try esc.refund(g.id, b) {
            okCount[keccak256("refund")]++;
            bool w1 = pre == CloakroomEscrow.State.Open && block.timestamp < t0;
            bool w2 = (pre == CloakroomEscrow.State.Open || pre == CloakroomEscrow.State.Ready)
                && block.timestamp >= t1 && block.timestamp < t2;
            bool w3 = pre == CloakroomEscrow.State.Locked && block.timestamp >= uint256(lockedAt) + w;
            if (!(w1 || w2 || w3)) _flag("refund outside its windows");
            if ((pre == CloakroomEscrow.State.Open || pre == CloakroomEscrow.State.Ready) && block.timestamp >= t2) {
                _flag("refund from Open/Ready at/after t2");
            }
            if (b != shares[g.bIdx]) _flag("refund with wrong scalar");
            if (g.claimed) _flag("refund after claim");
            g.refunded = true;
            settledCredited += amount;
        } catch {}
    }

    function withdraw(uint256 ownerSel) external {
        totalTries++;
        address owner = allOwner(ownerSel % 8);
        uint256 cr = esc.credit(address(tok), owner);
        uint256 bal0 = tok.balanceOf(owner);
        esc.withdraw(address(tok), owner);
        okCount[keccak256("withdraw")]++;
        if (tok.balanceOf(owner) - bal0 != cr) _flag("withdraw paid != credit");
        if (esc.credit(address(tok), owner) != 0) _flag("credit not cleared");
        withdrawn += cr;
    }

    function warp(uint256 dt) external {
        totalTries++;
        vm.warp(block.timestamp + bound(dt, 0, 75 minutes));
    }

    // ------------------------------------------------------------------ views for invariants

    /// All 8 addresses that can ever hold credit: 3 makers, 3 payouts, 2 relayers.
    function allOwner(uint256 i) public view returns (address) {
        if (i < 3) return makers[i];
        if (i < 6) return payoutAddr(i - 3);
        return relayers[i - 6];
    }

    function sumCredits() public view returns (uint256 s) {
        for (uint256 i; i < 8; i++) {
            s += esc.credit(address(tok), allOwner(i));
        }
    }

    function sumLive() public view returns (uint256 s) {
        for (uint256 i; i < ghosts.length; i++) {
            CloakroomEscrow.State st = _state(ghosts[i].id);
            if (st == CloakroomEscrow.State.Open || st == CloakroomEscrow.State.Ready || st == CloakroomEscrow.State.Locked)
            {
                (,,,,, uint256 amount,) = _times(ghosts[i].id);
                s += amount;
            }
        }
    }

    function stateOf(bytes32 id) external view returns (CloakroomEscrow.State) {
        return _state(id);
    }
}

contract CloakroomEscrowInvariantTest is StdInvariant, Test {
    EscrowHandler h;

    function setUp() public {
        h = new EscrowHandler();
        targetContract(address(h));
        bytes4[] memory sel = new bytes4[](7);
        sel[0] = EscrowHandler.open.selector;
        sel[1] = EscrowHandler.setReady.selector;
        sel[2] = EscrowHandler.commitClaim.selector;
        sel[3] = EscrowHandler.claim.selector;
        sel[4] = EscrowHandler.refund.selector;
        sel[5] = EscrowHandler.withdraw.selector;
        sel[6] = EscrowHandler.warp.selector;
        targetSelector(FuzzSelector({addr: address(h), selectors: sel}));
    }

    /// escrow balance == sum of unsettled amounts + sum of outstanding credits
    function invariant_conservation() public view {
        assertEq(h.tok().balanceOf(address(h.esc())), h.sumLive() + h.sumCredits());
    }

    /// credits never exceed what was deposited; credited + withdrawn == settled amounts
    function invariant_creditsBounded() public view {
        uint256 c = h.sumCredits();
        assertLe(c, h.deposited());
        assertEq(c + h.withdrawn(), h.settledCredited());
        assertLe(h.withdrawn(), h.deposited());
    }

    /// token supply conserved across all holders
    function invariant_supply() public view {
        InvToken t = h.tok();
        uint256 s = t.balanceOf(address(h.esc()));
        for (uint256 i; i < 8; i++) {
            s += t.balanceOf(h.allOwner(i));
        }
        assertEq(s, t.totalSupply());
    }

    /// at most one exit per swap; terminal states match the ghost record (absorbing)
    function invariant_exclusivityAndAbsorbing() public view {
        uint256 n = h.ghostCount();
        for (uint256 i; i < n; i++) {
            EscrowHandler.Ghost memory g = h.ghostAt(i);
            assertFalse(g.claimed && g.refunded);
            CloakroomEscrow.State st = h.stateOf(g.id);
            if (g.claimed) assertEq(uint8(st), uint8(CloakroomEscrow.State.Claimed));
            if (g.refunded) assertEq(uint8(st), uint8(CloakroomEscrow.State.Refunded));
            if (st == CloakroomEscrow.State.Claimed) assertTrue(g.claimed);
            if (st == CloakroomEscrow.State.Refunded) assertTrue(g.refunded);
        }
    }

    /// no claim without (Ready + valid commit) or a punish-path commit at/after t2; and every
    /// other per-call check in the handler (windows, signatures, guards, maker scoping)
    function invariant_noHandlerViolation() public view {
        assertFalse(h.violation(), h.violationWhy());
    }

    // Coverage statistics accumulated ACROSS runs (each run resets contract state, so the
    // totals are carried in process environment variables). Printed with -vv.
    function _acc(string memory k, uint256 v) internal returns (uint256 t) {
        t = vm.envOr(k, uint256(0)) + v;
        vm.setEnv(k, vm.toString(t));
    }

    function afterInvariant() external {
        string[7] memory ops = ["open", "setReady", "commitClaim", "claim", "refund", "withdraw", "punish"];
        emit log_named_uint("runs", _acc("CLK_INV_runs", 1));
        for (uint256 i; i < 7; i++) {
            uint256 v = i < 6 ? h.okCount(keccak256(bytes(ops[i]))) : h.punishLocks();
            emit log_named_uint(string(abi.encodePacked("successful ", ops[i])), _acc(string(abi.encodePacked("CLK_INV_", ops[i])), v));
        }
        emit log_named_uint("calls attempted", _acc("CLK_INV_calls", h.totalTries()));
    }
}
