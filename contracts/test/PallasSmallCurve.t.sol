// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

// Exhaustive verification of the EXACT text of src/Pallas.sol (dbl, maddG, mulG, isDiscreteLogOf,
// isOnCurve) on small prime-order a=0 curves. test/generated/PallasSmall*.sol are produced by
// audit/gen_harnesses.sh from src/Pallas.sol by substituting only the constants P, Q, B, GX, GY.
//   E1: y^2 = x^3 + 5 over F_223,  #E = 229 (prime), G = (3, 60)     [same B as Pallas; Q > P as for Pallas]
//   E2: y^2 = x^3 + 3 over F_4987, #E = 5023 (prime), G = (1, 2)     [Q > P]
// The reference is textbook affine arithmetic (independent code, Fermat inversion).

import {Test} from "forge-std/Test.sol";
import {PallasSmall223 as S1} from "./generated/PallasSmall223.sol";
import {PallasSmall4987 as S2} from "./generated/PallasSmall4987.sol";

library AffineRef {
    struct Pt {
        uint256 x;
        uint256 y;
        bool inf;
    }

    function inv(uint256 a, uint256 p) internal pure returns (uint256 r) {
        r = 1;
        uint256 e = p - 2;
        uint256 b = a % p;
        while (e > 0) {
            if (e & 1 == 1) r = mulmod(r, b, p);
            b = mulmod(b, b, p);
            e >>= 1;
        }
    }

    function add(Pt memory P1, Pt memory P2, uint256 p) internal pure returns (Pt memory) {
        if (P1.inf) return P2;
        if (P2.inf) return P1;
        uint256 l;
        if (P1.x == P2.x) {
            if ((P1.y + P2.y) % p == 0) return Pt(0, 0, true);
            // doubling, a = 0: l = 3x^2 / 2y
            l = mulmod(mulmod(3, mulmod(P1.x, P1.x, p), p), inv(mulmod(2, P1.y, p), p), p);
        } else {
            l = mulmod((P2.y + p - P1.y) % p, inv((P2.x + p - P1.x) % p, p), p);
        }
        uint256 x3 = (mulmod(l, l, p) + 2 * p - P1.x - P2.x) % p;
        uint256 y3 = (mulmod(l, (P1.x + p - x3) % p, p) + p - P1.y) % p;
        return Pt(x3, y3, false);
    }
}

contract PallasSmallCurveTest is Test {
    using AffineRef for AffineRef.Pt;

    // ---------------------------------------------------------------- E1 (exhaustive)

    function _table1() internal pure returns (AffineRef.Pt[] memory t) {
        t = new AffineRef.Pt[](S1.Q);
        t[0] = AffineRef.Pt(0, 0, true);
        AffineRef.Pt memory g = AffineRef.Pt(S1.GX, S1.GY, false);
        for (uint256 k = 1; k < S1.Q; k++) {
            t[k] = AffineRef.add(t[k - 1], g, S1.P);
        }
        // sanity: group order really is Q
        assert(AffineRef.add(t[S1.Q - 1], g, S1.P).inf);
    }

    function test_E1_tableIsTheWholeCurve() public pure {
        AffineRef.Pt[] memory t = _table1();
        uint256 onCurve;
        for (uint256 x; x < S1.P; x++) {
            for (uint256 y; y < S1.P; y++) {
                bool lib = S1.isOnCurve(x, y);
                bool ref = mulmod(y, y, S1.P) == addmod(mulmod(mulmod(x, x, S1.P), x, S1.P), S1.B, S1.P);
                assertEq(lib, ref);
                if (lib) onCurve++;
            }
        }
        assertEq(onCurve + 1, S1.Q); // every affine point is a multiple of G (prime order)
        for (uint256 k = 1; k < S1.Q; k++) {
            assertTrue(S1.isOnCurve(t[k].x, t[k].y));
        }
        // non-canonical coordinates rejected by isOnCurve
        assertFalse(S1.isOnCurve(S1.GX + S1.P, S1.GY));
        assertFalse(S1.isOnCurve(S1.GX, S1.GY + S1.P));
        // ... and by isDiscreteLogOf (audit I-01 fix), for every point and its +P aliases
        for (uint256 k = 1; k < S1.Q; k++) {
            assertFalse(S1.isDiscreteLogOf(k, t[k].x + S1.P, t[k].y));
            assertFalse(S1.isDiscreteLogOf(k, t[k].x, t[k].y + S1.P));
        }
    }

    /// For every scalar k in [0, Q + 40) and every one of the Q-1 affine curve points (x, y):
    ///   isDiscreteLogOf(k, x, y) == (0 < k < Q && [k]G == (x, y)).
    /// Split in chunks so forge runs them in parallel.
    function _exhaustE1(uint256 kFrom, uint256 kTo) internal pure {
        AffineRef.Pt[] memory t = _table1();
        for (uint256 k = kFrom; k < kTo; k++) {
            (uint256 X, uint256 Y, uint256 Z) = S1.mulG(k);
            if (k > 0 && k < S1.Q) {
                assertTrue(Z != 0, "mulG returned the identity for 0 < k < Q");
                // Jacobian result equals reference
                uint256 zi = AffineRef.inv(Z, S1.P);
                assertEq(mulmod(X, mulmod(zi, zi, S1.P), S1.P), t[k].x);
                assertEq(mulmod(Y, mulmod(mulmod(zi, zi, S1.P), zi, S1.P), S1.P), t[k].y);
            }
            for (uint256 j = 1; j < S1.Q; j++) {
                bool expect = k > 0 && k < S1.Q && j == k;
                assertEq(S1.isDiscreteLogOf(k, t[j].x, t[j].y), expect);
            }
            // (0,0), (0,1) and other off-curve pairs never match
            assertFalse(S1.isDiscreteLogOf(k, 0, 0));
            assertFalse(S1.isDiscreteLogOf(k, 0, 1));
        }
    }

    function test_E1_exhaustive_k000_040() public pure {
        _exhaustE1(0, 40);
    }

    function test_E1_exhaustive_k040_080() public pure {
        _exhaustE1(40, 80);
    }

    function test_E1_exhaustive_k080_120() public pure {
        _exhaustE1(80, 120);
    }

    function test_E1_exhaustive_k120_160() public pure {
        _exhaustE1(120, 160);
    }

    function test_E1_exhaustive_k160_200() public pure {
        _exhaustE1(160, 200);
    }

    function test_E1_exhaustive_k200_269() public pure {
        _exhaustE1(200, S1.Q + 40);
    }

    /// mulG on Q itself and on multiples of Q yields the identity (Z == 0), so the Z == 0 guard
    /// in isDiscreteLogOf is the only thing standing between k ≡ 0 (mod Q) and a bogus compare.
    function test_E1_multiplesOfQ_giveIdentity() public pure {
        for (uint256 m = 1; m < 6; m++) {
            (,, uint256 Z) = S1.mulG(m * S1.Q);
            assertEq(Z, 0);
        }
    }

    // ---------------------------------------------------------------- E2 (all scalars)

    function test_E2_allScalars() public pure {
        AffineRef.Pt memory g = AffineRef.Pt(S2.GX, S2.GY, false);
        AffineRef.Pt memory acc = AffineRef.Pt(0, 0, true);
        AffineRef.Pt memory prev;
        for (uint256 k = 1; k < S2.Q; k++) {
            prev = acc;
            acc = AffineRef.add(acc, g, S2.P);
            assertFalse(acc.inf);
            assertTrue(S2.isDiscreteLogOf(k, acc.x, acc.y));
            assertFalse(S2.isDiscreteLogOf(k, acc.x, (S2.P - acc.y) % S2.P)); // -[k]G (y != 0: odd order)
            if (k > 1) assertFalse(S2.isDiscreteLogOf(k, prev.x, prev.y));
            assertFalse(S2.isDiscreteLogOf(k + S2.Q, acc.x, acc.y)); // k >= Q rejected
        }
        assertTrue(AffineRef.add(acc, g, S2.P).inf);
        assertFalse(S2.isDiscreteLogOf(0, S2.GX, S2.GY));
        assertFalse(S2.isDiscreteLogOf(S2.Q, S2.GX, S2.GY));
    }
}
