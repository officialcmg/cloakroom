// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

// Regression checks for audit findings L-01 and I-01 (see audit/REPORT.md).
// Before the fixes these tests FAILED, reproducing each finding. Since the fixes they PASS:
//  - L-01: open() compares the escrow's balance before and after transferFrom and reverts
//    with UnsupportedToken unless it went up by exactly `amount`.
//  - I-01: Pallas.isDiscreteLogOf rejects x >= P or y >= P.

import {Test} from "forge-std/Test.sol";
import {CloakroomEscrow} from "../src/CloakroomEscrow.sol";
import {Pallas} from "../src/Pallas.sol";

/// 1% fee-on-transfer token (stands in for any token whose received amount != `amount`:
/// fee-on-transfer, deflationary, negatively rebasing, or an upgradeable token that adds a fee).
contract FeeOnTransferToken {
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

contract PallasFindingHarness {
    function check(uint256 k, uint256 x, uint256 y) external pure returns (bool) {
        return Pallas.isDiscreteLogOf(k, x, y);
    }
}

contract AuditFindingsTest is Test {
    uint256 constant AX = 0x17a182b1eb9594581ecaad0fcb61546f4c366cc57b9575ddde22ca56963c304c;
    uint256 constant AY = 0x19b252972f768cc5708b251baca77a9afd02dddd9531510aafb13c4824d7b61f;
    uint256 constant B_SEC = 0x2b8bbfabc504814096b330ffba3d828335e8db236f79aa7fded4d77640369e2a;
    uint256 constant BX = 0x183a6db8e5f566dfd863f5c0c0b846ecfb01d519efbd2259d829319dfd780e95;
    uint256 constant BY = 0x0b8b2c14d8675f49fedab2ee56d17c8547d6627cc8f83b2988e88fad65cb5972;

    /// L-01 (FIXED): open() used to credit `amount` instead of what actually arrived, so a
    /// fee-on-transfer token left the per-token credit pool short and the last withdrawer could
    /// not be paid. open() now reverts UnsupportedToken for such tokens, so no under-collateralised
    /// swap can be created.
    function test_FIXED_L01_feeOnTransferToken_rejectedAtOpen() public {
        CloakroomEscrow esc = new CloakroomEscrow();
        FeeOnTransferToken t = new FeeOnTransferToken();
        address m1 = address(0x111);
        uint256 amt = 1_000e6;
        uint64 t0 = uint64(block.timestamp + 1 hours);
        t.mint(m1, amt);
        vm.startPrank(m1);
        t.approve(address(esc), amt);
        vm.expectRevert(CloakroomEscrow.UnsupportedToken.selector);
        esc.open(bytes32("s"), address(0xAAA), address(t), amt, 0, AX, AY, BX, BY, t0, t0 + 1, t0 + 2, 1);
        vm.stopPrank();
        // nothing was recorded and nothing moved
        (,,,,,,,,, CloakroomEscrow.State st,,,,,,,) = esc.swaps(esc.swapId(m1, bytes32("s")));
        assertEq(uint8(st), uint8(CloakroomEscrow.State.None));
        assertEq(t.balanceOf(address(esc)), 0);
        assertEq(t.balanceOf(m1), amt);
    }

    /// L-01 (FIXED): fuzzed fee, 0 < fee < amount. Every such deposit is rejected.
    function testFuzz_FIXED_L01_anyShortfallRejected(uint256 amt) public {
        amt = bound(amt, 100, 1e30); // at least 100 units, so the 1% fee is nonzero
        CloakroomEscrow esc = new CloakroomEscrow();
        FeeOnTransferToken t = new FeeOnTransferToken();
        t.mint(address(this), amt);
        t.approve(address(esc), amt);
        uint64 t0 = uint64(block.timestamp + 1);
        vm.expectRevert(CloakroomEscrow.UnsupportedToken.selector);
        esc.open(bytes32("s"), address(0xAAA), address(t), amt, 0, AX, AY, BX, BY, t0, t0 + 1, t0 + 2, 1);
    }

    /// I-01 (FIXED): isDiscreteLogOf used to accept non-canonical (x + P, y), e.g. it reported
    /// [1]G == (GX + P, GY). This was unreachable through the escrow, because open() stores only
    /// points that pass isOnCurve, but it broke the library's documented contract. It now rejects
    /// x >= P and y >= P.
    function test_FIXED_I01_isDiscreteLogOf_rejectsNonCanonicalCoordinates() public {
        PallasFindingHarness h = new PallasFindingHarness();
        assertTrue(h.check(1, Pallas.GX, Pallas.GY));
        assertFalse(h.check(1, Pallas.GX + Pallas.P, Pallas.GY));
        assertFalse(h.check(1, Pallas.GX, Pallas.GY + Pallas.P));
        assertFalse(h.check(Pallas.Q - 1, Pallas.GX + Pallas.P, Pallas.P - Pallas.GY));
    }
}
