// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

// Audit property tests against the REAL src/CloakroomEscrow.sol + src/Pallas.sol (no stubs).
// All tests here PASS; intentionally failing bug reproductions live in test/AuditFindings.t.sol.

import {Test, Vm} from "forge-std/Test.sol";
import {VmSafe} from "forge-std/Vm.sol";
import {CloakroomEscrow} from "../src/CloakroomEscrow.sol";
import {Pallas} from "../src/Pallas.sol";

contract AuditToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 v) external {
        balanceOf[to] += v;
    }

    function approve(address s, uint256 v) external returns (bool) {
        allowance[msg.sender][s] = v;
        return true;
    }

    function transfer(address to, uint256 v) external virtual returns (bool) {
        balanceOf[msg.sender] -= v;
        balanceOf[to] += v;
        return true;
    }

    function transferFrom(address f, address to, uint256 v) external virtual returns (bool) {
        allowance[f][msg.sender] -= v;
        balanceOf[f] -= v;
        balanceOf[to] += v;
        return true;
    }
}

/// Malicious ERC20: re-enters the escrow from inside transfer / transferFrom.
contract ReentrantERC20 {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    CloakroomEscrow public esc;
    uint256 public bShare; // share b, to try a refund from inside open()
    bytes32 public openingId;
    uint256 public reentries;
    bool public sawOpenStateDuringPull;
    uint256 public creditSeenDuringPush = type(uint256).max;

    constructor(CloakroomEscrow e) {
        esc = e;
    }

    function setPlan(bytes32 id, uint256 b) external {
        openingId = id;
        bShare = b;
    }

    function mint(address to, uint256 v) external {
        balanceOf[to] += v;
    }

    function approve(address s, uint256 v) external returns (bool) {
        allowance[msg.sender][s] = v;
        return true;
    }

    function transferFrom(address f, address to, uint256 v) external returns (bool) {
        if (reentries == 0 && openingId != bytes32(0)) {
            reentries++;
            (,,,,,,,,, CloakroomEscrow.State st,,,,,,,) = esc.swaps(openingId);
            sawOpenStateDuringPull = st == CloakroomEscrow.State.Open;
            // try to refund the swap that is being opened, BEFORE the tokens arrive
            try esc.refund(openingId, bShare) {} catch {}
            // and drain it right away
            try esc.withdraw(address(this), f) {} catch {}
        }
        allowance[f][msg.sender] -= v;
        balanceOf[f] -= v;
        balanceOf[to] += v;
        return true;
    }

    function transfer(address to, uint256 v) external returns (bool) {
        if (reentries < 3) {
            reentries++;
            creditSeenDuringPush = esc.credit(address(this), to);
            esc.withdraw(address(this), to); // double-withdraw attempt
        }
        balanceOf[msg.sender] -= v;
        balanceOf[to] += v;
        return true;
    }
}

contract PallasGasHarness {
    function check(uint256 k, uint256 x, uint256 y) external pure returns (bool) {
        return Pallas.isDiscreteLogOf(k, x, y);
    }
}

contract CloakroomEscrowAuditTest is Test {
    uint256 constant A_SEC = 0x195661e2e4789b7f57137b2e777736d79720270638d12e4882fd95e52a73d6b3;
    uint256 constant AX = 0x17a182b1eb9594581ecaad0fcb61546f4c366cc57b9575ddde22ca56963c304c;
    uint256 constant AY = 0x19b252972f768cc5708b251baca77a9afd02dddd9531510aafb13c4824d7b61f;
    uint256 constant B_SEC = 0x2b8bbfabc504814096b330ffba3d828335e8db236f79aa7fded4d77640369e2a;
    uint256 constant BX = 0x183a6db8e5f566dfd863f5c0c0b846ecfb01d519efbd2259d829319dfd780e95;
    uint256 constant BY = 0x0b8b2c14d8675f49fedab2ee56d17c8547d6627cc8f83b2988e88fad65cb5972;
    uint256 constant SECP_N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141;

    CloakroomEscrow esc;
    AuditToken tok;
    address maker = address(0xB0B);
    uint256 payoutKey = 0xA11CE;
    address payout;
    address relayer = address(0xBEEF);
    uint256 constant AMT = 3_500e6;
    uint256 constant FEE = 1e6;
    uint64 constant W = 12 hours;
    uint64 t0;
    uint64 t1;
    uint64 t2;

    function setUp() public {
        esc = new CloakroomEscrow();
        tok = new AuditToken();
        payout = vm.addr(payoutKey);
        tok.mint(maker, 10 * AMT);
        vm.prank(maker);
        tok.approve(address(esc), type(uint256).max);
        t0 = uint64(block.timestamp + 1 hours);
        t1 = uint64(block.timestamp + 3 hours);
        t2 = uint64(block.timestamp + 5 hours);
    }

    // ---- single place that knows the open() ABI ----
    function _openWith(address mk, bytes32 salt, address token) internal returns (bytes32 id) {
        vm.prank(mk);
        id = esc.open(salt, payout, token, AMT, FEE, AX, AY, BX, BY, t0, t1, t2, W);
    }

    function _open() internal returns (bytes32) {
        return _openWith(maker, bytes32("s1"), address(tok));
    }

    function _commit(bytes32 id) internal {
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(payoutKey, esc.commitDigest(id, c, relayer));
        esc.commitClaim(id, c, relayer, v, r, s);
    }

    function _raw(bytes32 id) internal view returns (bytes32) {
        (bool ok, bytes memory ret) = address(esc).staticcall(abi.encodeWithSignature("swaps(bytes32)", id));
        require(ok);
        return keccak256(ret);
    }

    function _state(bytes32 id) internal view returns (CloakroomEscrow.State st) {
        (,,,,,,,,, st,,,,,,,) = esc.swaps(id);
    }

    /// Asserts that the escrow itself made NO account access of any kind (call, staticcall,
    /// delegatecall, create, balance/extcode reads) except to `allowed` (a precompile, or
    /// address(0) for none), and that the test made exactly one CALL into the escrow.
    function _assertOnlyEscrow(Vm.AccountAccess[] memory acc, address allowed) internal {
        _assertOnlyEscrowFrom(acc, allowed, address(this));
    }

    function _assertOnlyEscrowFrom(Vm.AccountAccess[] memory acc, address allowed, address top) internal {
        uint256 topCalls;
        for (uint256 i; i < acc.length; i++) {
            Vm.AccountAccess memory a = acc[i];
            if (a.kind == VmSafe.AccountAccessKind.Resume) continue;
            if (a.accessor == top || a.accessor == address(this)) {
                // the test's own accesses: the call into the escrow (+ its extcodesize check)
                // and cheatcode calls (vm.prank etc.)
                assertTrue(a.account == address(esc) || a.account == address(vm));
                if (a.account == address(esc) && a.kind == VmSafe.AccountAccessKind.Call) topCalls++;
                continue;
            }
            if (allowed != address(0) && a.account == allowed && a.accessor == address(esc)) continue;
            emit log_named_address("unexpected access to", a.account);
            emit log_named_address("by", a.accessor);
            emit log_named_uint("kind", uint256(a.kind));
            fail();
        }
        assertEq(topCalls, 1);
    }

    // ===================================================== claim / refund make no external calls

    function test_claim_noExternalCalls() public {
        bytes32 id = _open();
        vm.prank(maker);
        esc.setReady(id);
        _commit(id);
        vm.startStateDiffRecording();
        esc.claim(id, A_SEC);
        _assertOnlyEscrow(vm.stopAndReturnStateDiff(), address(0));
    }

    function test_claim_punishPath_noExternalCalls() public {
        bytes32 id = _open();
        vm.warp(t2);
        _commit(id);
        vm.startStateDiffRecording();
        esc.claim(id, A_SEC);
        _assertOnlyEscrow(vm.stopAndReturnStateDiff(), address(0));
    }

    function test_refund_allWindows_noExternalCalls() public {
        // window 1: Open, before t0
        bytes32 id1 = _openWith(maker, "w1", address(tok));
        vm.startStateDiffRecording();
        esc.refund(id1, B_SEC);
        _assertOnlyEscrow(vm.stopAndReturnStateDiff(), address(0));
        // window 2: Ready, in [t1, t2)
        bytes32 id2 = _openWith(maker, "w2", address(tok));
        vm.prank(maker);
        esc.setReady(id2);
        // window 3: Locked, after lockedAt + W
        bytes32 id3 = _openWith(maker, "w3", address(tok));
        vm.prank(maker);
        esc.setReady(id3);
        _commit(id3);
        vm.warp(t1);
        vm.startStateDiffRecording();
        esc.refund(id2, B_SEC);
        _assertOnlyEscrow(vm.stopAndReturnStateDiff(), address(0));
        vm.warp(block.timestamp + W);
        vm.startStateDiffRecording();
        esc.refund(id3, B_SEC);
        _assertOnlyEscrow(vm.stopAndReturnStateDiff(), address(0));
    }

    function test_setReady_and_commitClaim_externalCalls() public {
        bytes32 id = _open();
        vm.prank(maker);
        vm.startStateDiffRecording();
        esc.setReady(id);
        _assertOnlyEscrowFrom(vm.stopAndReturnStateDiff(), address(0), maker);
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(payoutKey, esc.commitDigest(id, c, relayer));
        vm.startStateDiffRecording();
        esc.commitClaim(id, c, relayer, v, r, s);
        _assertOnlyEscrow(vm.stopAndReturnStateDiff(), address(1)); // only the ecrecover precompile
    }

    /// Detector sanity: the same recording DOES see the token call made by withdraw().
    function test_detectorSanity_withdrawTokenCallIsSeen() public {
        bytes32 id = _open();
        esc.refund(id, B_SEC);
        vm.startStateDiffRecording();
        esc.withdraw(address(tok), maker);
        Vm.AccountAccess[] memory acc = vm.stopAndReturnStateDiff();
        uint256 n;
        for (uint256 i; i < acc.length; i++) {
            if (acc[i].accessor == address(esc) && acc[i].account == address(tok) && acc[i].kind == VmSafe.AccountAccessKind.Call) n++;
        }
        assertEq(n, 1);
    }

    // ===================================================== reentrancy with a malicious ERC20

    function test_reentrancy_open_refundDuringPull() public {
        ReentrantERC20 rt = new ReentrantERC20(esc);
        rt.mint(maker, AMT);
        vm.prank(maker);
        rt.approve(address(esc), AMT);
        bytes32 id = esc.swapId(maker, "re");
        rt.setPlan(id, B_SEC);
        _openWith(maker, "re", address(rt));
        // effects were applied before the external call
        assertTrue(rt.sawOpenStateDuringPull());
        // the nested refund+withdraw happened, but accounting is still exact: the maker was paid
        // `AMT` once (from the tokens being pulled in the same call), escrow holds nothing.
        assertEq(uint8(_state(id)), uint8(CloakroomEscrow.State.Refunded));
        assertEq(rt.balanceOf(address(esc)) + rt.balanceOf(maker), AMT);
        assertEq(esc.credit(address(rt), maker), rt.balanceOf(address(esc)));
    }

    function test_reentrancy_withdraw_noDoublePay() public {
        ReentrantERC20 rt = new ReentrantERC20(esc);
        rt.mint(maker, 2 * AMT);
        vm.prank(maker);
        rt.approve(address(esc), 2 * AMT);
        bytes32 id = _openWith(maker, "a", address(rt));
        _openWith(maker, "b", address(rt)); // second swap keeps a balance in the escrow to steal
        esc.refund(id, B_SEC);
        assertEq(esc.credit(address(rt), maker), AMT);
        uint256 before = rt.balanceOf(maker);
        esc.withdraw(address(rt), maker);
        assertEq(rt.creditSeenDuringPush(), 0); // credit zeroed before the transfer
        assertEq(rt.balanceOf(maker) - before, AMT); // paid once, not twice
        assertEq(rt.balanceOf(address(esc)), AMT); // second swap still fully backed
    }

    // ===================================================== maker scoping

    function testFuzz_makerScoping(address attacker, bytes32 salt, bytes32 victimSalt) public {
        vm.assume(attacker != maker && attacker != address(0) && attacker != address(esc));
        bytes32 vid = _openWith(maker, victimSalt, address(tok));
        bytes32 before = _raw(vid);
        tok.mint(attacker, AMT);
        vm.prank(attacker);
        tok.approve(address(esc), AMT);
        bytes32 aid = _openWith(attacker, salt, address(tok));
        assertTrue(aid != vid);
        assertEq(aid, esc.swapId(attacker, salt));
        assertEq(_raw(vid), before);
        // victim cannot be re-opened either, not even by the victim
        vm.prank(maker);
        vm.expectRevert(CloakroomEscrow.BadState.selector);
        esc.open(victimSalt, payout, address(tok), AMT, FEE, AX, AY, BX, BY, t0, t1, t2, W);
    }

    // ===================================================== signature edge cases (informational)

    /// ecrecover accepts the malleable (v^1, n - s) twin. Harmless: it recovers the same payout
    /// signer over the same digest, and a swap can be locked only once.
    function test_info_malleableSignatureAccepted() public {
        bytes32 id = _open();
        vm.prank(maker);
        esc.setReady(id);
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(payoutKey, esc.commitDigest(id, c, relayer));
        esc.commitClaim(id, c, relayer, v == 27 ? 28 : 27, r, bytes32(SECP_N - uint256(s)));
        assertEq(uint8(_state(id)), uint8(CloakroomEscrow.State.Locked));
    }

    /// Commit signatures carry no expiry. A commit that arrives late (in [t1, t2)) reverts, but its
    /// calldata is public, and anyone can replay the same signature from t2 to lock the swap
    /// via the punish path. (Informational: the lock only helps the taker, who already signed it;
    /// it can delay the maker's refund by W if the taker then never reveals.)
    function test_info_lateCommitSignatureReplayableAtT2() public {
        bytes32 id = _open();
        vm.prank(maker);
        esc.setReady(id);
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(payoutKey, esc.commitDigest(id, c, relayer));
        vm.warp(t1 + 1);
        vm.expectRevert(CloakroomEscrow.NotClaimable.selector);
        esc.commitClaim(id, c, relayer, v, r, s);
        vm.warp(t2);
        vm.prank(address(0xDEAD)); // unrelated third party
        esc.commitClaim(id, c, relayer, v, r, s);
        assertEq(uint8(_state(id)), uint8(CloakroomEscrow.State.Locked));
        // maker's refund now waits until lockedAt + W
        vm.expectRevert(CloakroomEscrow.TooEarly.selector);
        esc.refund(id, B_SEC);
        vm.warp(t2 + W);
        esc.refund(id, B_SEC);
    }

    /// commitClaim from Open is impossible before t2, at every instant (fuzzed time).
    function testFuzz_commitFromOpenOnlyFromT2(uint64 ts) public {
        bytes32 id = _open();
        ts = uint64(bound(ts, block.timestamp, t2 + 10 days));
        vm.warp(ts);
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(payoutKey, esc.commitDigest(id, c, relayer));
        (bool ok,) = address(esc).call(abi.encodeCall(esc.commitClaim, (id, c, relayer, v, r, s)));
        assertEq(ok, ts >= t2);
    }

    /// refund from Open/Ready never succeeds at or after t2 (fuzzed time, both states).
    function testFuzz_refundOpenReadyNeverFromT2(uint64 ts, bool ready) public {
        bytes32 id = _open();
        if (ready) {
            vm.prank(maker);
            esc.setReady(id);
        }
        ts = uint64(bound(ts, block.timestamp, t2 + 10 days));
        vm.warp(ts);
        (bool ok,) = address(esc).call(abi.encodeCall(esc.refund, (id, B_SEC)));
        bool expected = (!ready && ts < t0) || (ts >= t1 && ts < t2);
        assertEq(ok, expected);
    }

    // ===================================================== gas: scalar dependence of the Pallas check

    function test_gas_pallasScalarDependence() public {
        PallasGasHarness g = new PallasGasHarness();
        uint256 Q = Pallas.Q;
        uint256[10] memory ks = [
            uint256(1),
            2,
            3,
            uint256(1) << 128,
            uint256(1) << 254, // single top bit (< Q)
            (uint256(1) << 254) - 1, // 254 ones
            Q - 1, // top bit + dense low half
            A_SEC,
            B_SEC,
            0x1555555555555555555555555555555555555555555555555555555555555555
        ];
        uint256 mn = type(uint256).max;
        uint256 mx;
        uint256 sum;
        uint256 n;
        uint256 rmn = type(uint256).max;
        uint256 rmx;
        for (uint256 i; i < ks.length; i++) {
            uint256 g0 = gasleft();
            g.check(ks[i], AX, AY);
            uint256 used = g0 - gasleft();
            emit log_named_uint(string(abi.encodePacked("gas k#", vm.toString(i))), used);
            if (used < mn) mn = used;
            if (used > mx) mx = used;
        }
        // 256 pseudo-random full-size scalars in [1, Q)
        for (uint256 i; i < 256; i++) {
            uint256 k = uint256(keccak256(abi.encode("k", i))) % (Q - 1) + 1;
            uint256 g0 = gasleft();
            g.check(k, AX, AY);
            uint256 used = g0 - gasleft();
            sum += used;
            n++;
            if (used < rmn) rmn = used;
            if (used > rmx) rmx = used;
            if (used < mn) mn = used;
            if (used > mx) mx = used;
        }
        emit log_named_uint("pallas check min (all samples)", mn);
        emit log_named_uint("pallas check max (all samples)", mx);
        emit log_named_uint("pallas check avg (256 random)", sum / n);
        emit log_named_uint("pallas check min (256 random)", rmn);
        emit log_named_uint("pallas check max (256 random)", rmx);
        // rejected scalars short-circuit
        uint256 g1 = gasleft();
        g.check(0, AX, AY);
        emit log_named_uint("pallas check k=0 (rejected)", g1 - gasleft());
        g1 = gasleft();
        g.check(Q, AX, AY);
        emit log_named_uint("pallas check k=Q (rejected)", g1 - gasleft());
    }
}
