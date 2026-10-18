// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test, Vm} from "forge-std/Test.sol";
import {CloakroomEscrow} from "../src/CloakroomEscrow.sol";
import {Pallas} from "../src/Pallas.sol";

/// Test token with an issuer pause switch, standing in for USDC's pause/blocklist powers.
contract PausableToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    bool public paused;

    function setPaused(bool p) external {
        paused = p;
    }

    function mint(address to, uint256 v) external {
        balanceOf[to] += v;
    }

    function approve(address s, uint256 v) external returns (bool) {
        allowance[msg.sender][s] = v;
        return true;
    }

    function transfer(address to, uint256 v) external returns (bool) {
        require(!paused, "paused");
        balanceOf[msg.sender] -= v;
        balanceOf[to] += v;
        return true;
    }

    function transferFrom(address f, address to, uint256 v) external returns (bool) {
        require(!paused, "paused");
        allowance[f][msg.sender] -= v;
        balanceOf[f] -= v;
        balanceOf[to] += v;
        return true;
    }
}

/// Delivers 1% less than requested (fee-on-transfer / deflationary token).
contract FeeToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 v) external {
        balanceOf[to] += v;
    }

    function approve(address s, uint256 v) external returns (bool) {
        allowance[msg.sender][s] = v;
        return true;
    }

    function transfer(address to, uint256 v) external returns (bool) {
        balanceOf[msg.sender] -= v;
        balanceOf[to] += v - v / 100;
        return true;
    }

    function transferFrom(address f, address to, uint256 v) external returns (bool) {
        allowance[f][msg.sender] -= v;
        balanceOf[f] -= v;
        balanceOf[to] += v - v / 100;
        return true;
    }
}

contract PallasHarness {
    function check(uint256 k, uint256 x, uint256 y) external pure returns (bool) {
        return Pallas.isDiscreteLogOf(k, x, y);
    }

    function onCurve(uint256 x, uint256 y) external pure returns (bool) {
        return Pallas.isOnCurve(x, y);
    }
}

contract CloakroomEscrowTest is Test {
    // Vectors produced by keys-spike against orchard 0.16.0 (post-NU6.3 key rules).
    uint256 constant A_SEC = 0x195661e2e4789b7f57137b2e777736d79720270638d12e4882fd95e52a73d6b3;
    uint256 constant AX = 0x17a182b1eb9594581ecaad0fcb61546f4c366cc57b9575ddde22ca56963c304c;
    uint256 constant AY = 0x19b252972f768cc5708b251baca77a9afd02dddd9531510aafb13c4824d7b61f;
    uint256 constant B_SEC = 0x2b8bbfabc504814096b330ffba3d828335e8db236f79aa7fded4d77640369e2a;
    uint256 constant BX = 0x183a6db8e5f566dfd863f5c0c0b846ecfb01d519efbd2259d829319dfd780e95;
    uint256 constant BY = 0x0b8b2c14d8675f49fedab2ee56d17c8547d6627cc8f83b2988e88fad65cb5972;

    CloakroomEscrow esc;
    PausableToken usdc;
    PallasHarness h;
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
        usdc = new PausableToken();
        h = new PallasHarness();
        payout = vm.addr(payoutKey);
        usdc.mint(maker, AMT);
        vm.prank(maker);
        usdc.approve(address(esc), AMT);
        t0 = uint64(block.timestamp + 1 hours);
        t1 = uint64(block.timestamp + 3 hours);
        t2 = uint64(block.timestamp + 5 hours);
    }

    function _open() internal returns (bytes32 id) {
        vm.prank(maker);
        id = esc.open(bytes32("s1"), payout, address(usdc), AMT, FEE, AX, AY, BX, BY, t0, t1, t2, W);
    }

    function _ready(bytes32 id) internal {
        vm.prank(maker);
        esc.setReady(id);
    }

    function _sig(uint256 key, bytes32 id, bytes32 c, address rel) internal view returns (uint8 v, bytes32 r, bytes32 s) {
        (v, r, s) = vm.sign(key, esc.commitDigest(id, c, rel));
    }

    function _commit(bytes32 id) internal {
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = _sig(payoutKey, id, c, relayer);
        vm.prank(relayer);
        esc.commitClaim(id, c, relayer, v, r, s);
    }

    function _state(bytes32 id) internal view returns (CloakroomEscrow.State st) {
        (,,,,,,,,, st,,,,,,,) = esc.swaps(id);
    }

    // ------------------------------------------------ curve arithmetic (unchanged library)

    function test_vectorsMatchRust() public view {
        assertTrue(h.onCurve(Pallas.GX, Pallas.GY));
        assertTrue(h.check(1, Pallas.GX, Pallas.GY));
        assertTrue(h.check(A_SEC, AX, AY));
        assertTrue(h.check(B_SEC, BX, BY));
        assertFalse(h.check(A_SEC, BX, BY));
        assertTrue(h.check(Pallas.Q - 1, Pallas.GX, Pallas.P - Pallas.GY));
    }

    function test_edgeScalarsRejected() public view {
        uint256[7] memory bad = [uint256(0), Pallas.Q, Pallas.Q + 1, type(uint256).max, uint256(2), Pallas.Q - 2, uint256(1)];
        for (uint256 i; i < bad.length; i++) {
            assertFalse(h.check(bad[i], AX, AY));
            assertFalse(h.check(bad[i], BX, BY));
        }
        assertFalse(h.check(0, 0, 0));
        assertFalse(h.onCurve(0, 0));
    }

    /// k and k + Q name the same point; only the canonical scalar may open a commitment.
    function test_scalarAliasesRejected() public view {
        assertTrue(h.check(1, Pallas.GX, Pallas.GY));
        assertFalse(h.check(1 + Pallas.Q, Pallas.GX, Pallas.GY));
        assertFalse(h.check(A_SEC + Pallas.Q, AX, AY));
        assertFalse(h.check(B_SEC + Pallas.Q, BX, BY));
    }

    function testFuzz_randomScalarNeverMatches(uint256 k) public view {
        vm.assume(k != A_SEC);
        assertFalse(h.check(k, AX, AY));
    }

    // ------------------------------------------------ happy path

    function test_happyPath_commitRevealWithdraw() public {
        bytes32 id = _open();
        _ready(id);
        uint256 g0 = gasleft();
        _commit(id);
        emit log_named_uint("gas: commitClaim (incl. ecrecover)", g0 - gasleft());
        assertEq(uint8(_state(id)), uint8(CloakroomEscrow.State.Locked));
        g0 = gasleft();
        vm.prank(relayer);
        esc.claim(id, A_SEC);
        emit log_named_uint("gas: claim (Pallas check, no transfers)", g0 - gasleft());
        assertEq(esc.credit(address(usdc), payout), AMT - FEE);
        assertEq(esc.credit(address(usdc), relayer), FEE);
        esc.withdraw(address(usdc), payout);
        esc.withdraw(address(usdc), relayer);
        assertEq(usdc.balanceOf(payout), AMT - FEE);
        assertEq(usdc.balanceOf(relayer), FEE);
        vm.expectRevert(CloakroomEscrow.BadState.selector);
        esc.claim(id, A_SEC);
        vm.warp(t1 + W + 1);
        vm.expectRevert(CloakroomEscrow.TooEarly.selector);
        esc.refund(id, B_SEC);
    }

    // ------------------------------------------------ review finding 1: no unfunded claim

    /// Maker offline at t0 and never set Ready (no funding seen): the taker cannot lock or claim.
    function test_noClaimWithoutReady_evenAfterT0() public {
        bytes32 id = _open();
        vm.warp(t0 + 1);
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = _sig(payoutKey, id, c, relayer);
        vm.expectRevert(CloakroomEscrow.NotClaimable.selector);
        esc.commitClaim(id, c, relayer, v, r, s);
        vm.expectRevert(CloakroomEscrow.BadState.selector);
        esc.claim(id, A_SEC);
        // maker returns in [t1, t2) and is refunded in full
        vm.warp(t1);
        esc.refund(id, B_SEC);
        esc.withdraw(address(usdc), maker);
        assertEq(usdc.balanceOf(maker), AMT);
    }

    // ------------------------------------------------ review finding 2/3: late or failed calls leak nothing

    /// A commit that lands after t1 reverts; only a hash was in it, and the maker refunds.
    function test_lateCommitRevertsAndRefundFollows() public {
        bytes32 id = _open();
        _ready(id);
        vm.warp(t1);
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = _sig(payoutKey, id, c, relayer);
        vm.expectRevert(CloakroomEscrow.NotClaimable.selector);
        esc.commitClaim(id, c, relayer, v, r, s);
        esc.refund(id, B_SEC);
        assertEq(uint8(_state(id)), uint8(CloakroomEscrow.State.Refunded));
    }

    /// Token paused at claim time: claim still succeeds (credit only); withdraw waits for unpause.
    function test_tokenPauseCannotRevertClaim() public {
        bytes32 id = _open();
        _ready(id);
        _commit(id);
        usdc.setPaused(true);
        esc.claim(id, A_SEC); // must not revert: a revert here would publish a for nothing
        assertEq(uint8(_state(id)), uint8(CloakroomEscrow.State.Claimed));
        vm.expectRevert();
        esc.withdraw(address(usdc), payout);
        usdc.setPaused(false);
        esc.withdraw(address(usdc), payout);
        assertEq(usdc.balanceOf(payout), AMT - FEE);
    }

    /// Locked but never revealed: after the reveal window the maker refunds; the reveal is then closed.
    function test_lockedWithoutRevealRefundsAfterWindow() public {
        bytes32 id = _open();
        _ready(id);
        _commit(id);
        vm.warp(block.timestamp + W - 1);
        vm.expectRevert(CloakroomEscrow.TooEarly.selector);
        esc.refund(id, B_SEC);
        vm.warp(block.timestamp + 1);
        vm.expectRevert(CloakroomEscrow.TooLate.selector);
        esc.claim(id, A_SEC);
        esc.refund(id, B_SEC);
        esc.withdraw(address(usdc), maker);
        assertEq(usdc.balanceOf(maker), AMT);
    }

    /// Ready but no commit by t1 (taker vanished): no refund before t1, refund from t1.
    function test_readyNoCommit_refundOnlyFromT1() public {
        bytes32 id = _open();
        _ready(id);
        vm.warp(t1 - 1);
        vm.expectRevert(CloakroomEscrow.TooEarly.selector);
        esc.refund(id, B_SEC);
        vm.warp(t1);
        vm.recordLogs();
        esc.refund(id, B_SEC);
        Vm.Log[] memory logs = vm.getRecordedLogs();
        bool seen;
        for (uint256 i; i < logs.length; i++) {
            if (logs[i].topics[0] == CloakroomEscrow.Refunded.selector) {
                assertEq(abi.decode(logs[i].data, (uint256)), B_SEC);
                seen = true;
            }
        }
        assertTrue(seen);
    }

    /// Early refund (Open, before t0) still works for an unfunded swap.
    function test_earlyRefundBeforeT0() public {
        bytes32 id = _open();
        esc.refund(id, B_SEC);
        esc.withdraw(address(usdc), maker);
        assertEq(usdc.balanceOf(maker), AMT);
    }

    // ------------------------------------------------ review finding 5: relayer binding and griefing

    function test_commitNeedsPayoutSignature() public {
        bytes32 id = _open();
        _ready(id);
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = _sig(0xBAD, id, c, relayer); // not the payout key
        vm.expectRevert(CloakroomEscrow.BadSignature.selector);
        esc.commitClaim(id, c, relayer, v, r, s);
        // a valid signature cannot be replayed with a different relayer (fee sniping)
        (v, r, s) = _sig(payoutKey, id, c, relayer);
        vm.expectRevert(CloakroomEscrow.BadSignature.selector);
        esc.commitClaim(id, c, address(0x5A1), v, r, s);
        // nor with a different commitment (griefing lock with junk)
        vm.expectRevert(CloakroomEscrow.BadSignature.selector);
        esc.commitClaim(id, bytes32(uint256(1)), relayer, v, r, s);
    }

    function test_revealMustMatchCommitment() public {
        bytes32 id = _open();
        _ready(id);
        _commit(id);
        vm.expectRevert(CloakroomEscrow.BadSecret.selector);
        esc.claim(id, B_SEC);
        vm.expectRevert(CloakroomEscrow.BadSecret.selector);
        esc.claim(id, A_SEC + 1);
    }

    function test_wrongShareAtRefundDoor() public {
        bytes32 id = _open();
        vm.expectRevert(CloakroomEscrow.BadSecret.selector);
        esc.refund(id, A_SEC);
    }

    function test_guards() public {
        bytes32 id = _open();
        vm.expectRevert(CloakroomEscrow.NotMaker.selector);
        esc.setReady(id);
        vm.warp(t0);
        vm.prank(maker);
        vm.expectRevert(CloakroomEscrow.TooLate.selector);
        esc.setReady(id);
        vm.prank(maker);
        vm.expectRevert(CloakroomEscrow.BadParams.selector);
        esc.open(bytes32("x"), payout, address(usdc), AMT, AMT + 1, AX, AY, BX, BY, t0 + 10, t1 + 10, t2 + 10, W);
        vm.prank(maker);
        vm.expectRevert(CloakroomEscrow.BadParams.selector);
        esc.open(bytes32("y"), payout, address(usdc), AMT, FEE, AX, AY + 1, BX, BY, t0 + 10, t1 + 10, t2 + 10, W);
        vm.prank(maker);
        vm.expectRevert(CloakroomEscrow.BadState.selector);
        esc.open(bytes32("s1"), payout, address(usdc), AMT, FEE, AX, AY, BX, BY, t0 + 10, t1 + 10, t2 + 10, W);
    }

    /// Audit L-01 regression: a token that delivers less than `amount` is rejected at open().
    function test_feeOnTransferTokenRejected() public {
        FeeToken ft = new FeeToken();
        ft.mint(maker, AMT);
        vm.startPrank(maker);
        ft.approve(address(esc), AMT);
        vm.expectRevert(CloakroomEscrow.UnsupportedToken.selector);
        esc.open(bytes32("fot"), payout, address(ft), AMT, FEE, AX, AY, BX, BY, t0, t1, t2, W);
        vm.stopPrank();
    }

    /// Audit I-01 regression: non-canonical coordinates (x + P) are rejected by the library.
    function test_nonCanonicalCoordinatesRejected() public view {
        assertFalse(h.check(A_SEC, AX + Pallas.P, AY));
        assertFalse(h.check(A_SEC, AX, AY + Pallas.P));
    }

    function test_withdrawIsIdempotent() public {
        bytes32 id = _open();
        esc.refund(id, B_SEC);
        esc.withdraw(address(usdc), maker);
        esc.withdraw(address(usdc), maker); // second call moves nothing
        assertEq(usdc.balanceOf(maker), AMT);
    }

    // ------------------------------------------------ punish window t2

    /// Maker vanished before Ready: from t2 the taker may lock and claim (no hostage).
    function test_punishPathFromOpenAfterT2() public {
        bytes32 id = _open();
        vm.warp(t2);
        _commit(id);
        esc.claim(id, A_SEC);
        esc.withdraw(address(usdc), payout);
        assertEq(usdc.balanceOf(payout), AMT - FEE);
    }

    /// Ready but the taker missed t1, and the maker never refunded: the taker completes after t2.
    function test_punishPathFromReadyAfterT2() public {
        bytes32 id = _open();
        _ready(id);
        vm.warp(t2 + 1);
        _commit(id);
        esc.claim(id, A_SEC);
        assertEq(uint8(_state(id)), uint8(CloakroomEscrow.State.Claimed));
    }

    /// Between t1 and t2 a commit is impossible; the maker's refund window is exactly [t1, t2).
    function test_gapBetweenT1andT2() public {
        bytes32 id = _open();
        _ready(id);
        vm.warp(t1 + 1);
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = _sig(payoutKey, id, c, relayer);
        vm.expectRevert(CloakroomEscrow.NotClaimable.selector);
        esc.commitClaim(id, c, relayer, v, r, s);
        vm.warp(t2);
        vm.expectRevert(CloakroomEscrow.TooEarly.selector); // refund window closed at t2
        esc.refund(id, B_SEC);
    }

    /// A maker who refunds in [t1, t2) blocks the punish path (honest maker with L_M is safe).
    function test_makerRefundInWindowBlocksPunish() public {
        bytes32 id = _open();
        vm.warp(t2 - 1);
        esc.refund(id, B_SEC);
        vm.warp(t2);
        bytes32 c = esc.commitmentOf(id, A_SEC);
        (uint8 v, bytes32 r, bytes32 s) = _sig(payoutKey, id, c, relayer);
        vm.expectRevert(CloakroomEscrow.BadState.selector);
        esc.commitClaim(id, c, relayer, v, r, s);
    }

    // ------------------------------------------------ invariant: exactly one exit, supply conserved

    function testFuzz_exactlyOneExit(uint64 warp1, uint64 warp2, uint64 warp3, uint8 path) public {
        bytes32 id = _open();
        bool ready;
        bool committed;
        uint256 commitTime;
        if (path & 1 == 1) {
            vm.warp(bound(warp1, block.timestamp, t0 + 1));
            try this.readyAs(id) {ready = true;} catch {}
        }
        if (path & 2 == 2) {
            vm.warp(bound(warp2, block.timestamp, t2 + 1 hours));
            bytes32 c = esc.commitmentOf(id, A_SEC);
            (uint8 v, bytes32 r, bytes32 s) = _sig(payoutKey, id, c, relayer);
            try esc.commitClaim(id, c, relayer, v, r, s) {committed = true; commitTime = block.timestamp;} catch {}
        }
        vm.warp(bound(warp3, block.timestamp, t2 + 2 * W));
        bool claimed;
        bool refunded;
        if (path & 4 == 4) {
            try esc.claim(id, A_SEC) {claimed = true;} catch {}
            try esc.refund(id, B_SEC) {refunded = true;} catch {}
        } else {
            try esc.refund(id, B_SEC) {refunded = true;} catch {}
            try esc.claim(id, A_SEC) {claimed = true;} catch {}
        }
        assertFalse(claimed && refunded);
        // a claim needs a payout-signed commit made either while Ready before t1, or from t2
        if (claimed) {
            assertTrue(committed);
            assertTrue((ready && commitTime < t1) || commitTime >= t2);
        }
        esc.withdraw(address(usdc), payout);
        esc.withdraw(address(usdc), relayer);
        esc.withdraw(address(usdc), maker);
        uint256 total = usdc.balanceOf(payout) + usdc.balanceOf(relayer) + usdc.balanceOf(maker) + usdc.balanceOf(address(esc));
        assertEq(total, AMT);
    }

    function readyAs(bytes32 id) external {
        vm.prank(maker);
        esc.setReady(id);
    }
}
