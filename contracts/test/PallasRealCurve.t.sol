// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

// Real-curve (Pallas) consistency fuzzing of src/Pallas.sol against independent affine arithmetic:
// mulG must be a group homomorphism Z_Q -> E(F_p). A wrong doubling/addition formula, a wrong
// exceptional-case branch or a wrong projective comparison breaks this for almost all inputs.

import {Test} from "forge-std/Test.sol";
import {Pallas} from "../src/Pallas.sol";

contract PallasRealCurveTest is Test {
    uint256 constant P = Pallas.P;
    uint256 constant Q = Pallas.Q;

    function _inv(uint256 a) internal view returns (uint256 r) {
        (bool ok, bytes memory out) =
            address(5).staticcall(abi.encodePacked(uint256(32), uint256(32), uint256(32), a, P - 2, P));
        require(ok);
        r = abi.decode(out, (uint256));
    }

    function _affine(uint256 k) internal view returns (uint256 x, uint256 y) {
        (uint256 X, uint256 Y, uint256 Z) = Pallas.mulG(k);
        require(Z != 0, "identity");
        uint256 zi = _inv(Z);
        uint256 zi2 = mulmod(zi, zi, P);
        x = mulmod(X, zi2, P);
        y = mulmod(Y, mulmod(zi2, zi, P), P);
    }

    /// independent affine addition (a = 0), returns inf = true for the identity
    function _add(uint256 x1, uint256 y1, uint256 x2, uint256 y2) internal view returns (uint256 x3, uint256 y3, bool inf) {
        uint256 l;
        if (x1 == x2) {
            if (addmod(y1, y2, P) == 0) return (0, 0, true);
            l = mulmod(mulmod(3, mulmod(x1, x1, P), P), _inv(addmod(y1, y1, P)), P);
        } else {
            l = mulmod(addmod(y2, P - y1, P), _inv(addmod(x2, P - x1, P)), P);
        }
        x3 = addmod(mulmod(l, l, P), P - addmod(x1, x2, P), P);
        y3 = addmod(mulmod(l, addmod(x1, P - x3, P), P), P - y1, P);
    }

    /// forge-config: default.fuzz.runs = 2000
    function testFuzz_homomorphism(uint256 k1, uint256 k2) public view {
        k1 = bound(k1, 1, Q - 1);
        k2 = bound(k2, 1, Q - 1);
        uint256 k3 = addmod(k1, k2, Q);
        (uint256 x1, uint256 y1) = _affine(k1);
        (uint256 x2, uint256 y2) = _affine(k2);
        assertTrue(Pallas.isOnCurve(x1, y1));
        assertTrue(Pallas.isDiscreteLogOf(k1, x1, y1));
        (uint256 x3, uint256 y3, bool inf) = _add(x1, y1, x2, y2);
        if (k3 == 0) {
            assertTrue(inf);
            return;
        }
        assertFalse(inf);
        assertTrue(Pallas.isOnCurve(x3, y3));
        assertTrue(Pallas.isDiscreteLogOf(k3, x3, y3)); // [k1]G + [k2]G == [k1 + k2]G
        assertFalse(Pallas.isDiscreteLogOf(k3, x3, P - y3)); // and not its negation
        if (k1 != k3) assertFalse(Pallas.isDiscreteLogOf(k1, x3, y3));
    }

    /// forge-config: default.fuzz.runs = 1000
    function testFuzz_negationAndDoubling(uint256 k) public view {
        k = bound(k, 1, Q - 1);
        (uint256 x, uint256 y) = _affine(k);
        assertTrue(Pallas.isDiscreteLogOf(Q - k, x, P - y)); // [Q - k]G == -[k]G
        (uint256 x2, uint256 y2,) = _add(x, y, x, y);
        assertTrue(Pallas.isDiscreteLogOf(mulmod(2, k, Q), x2, y2)); // doubling
    }

    /// Structured scalars: every single-bit, every all-ones prefix, and their neighbours.
    function test_structuredScalars() public view {
        for (uint256 i; i < 255; i++) {
            uint256[3] memory ks = [uint256(1) << i, (uint256(1) << i) - 1, (uint256(1) << i) + 1];
            for (uint256 j; j < 3; j++) {
                uint256 k = ks[j];
                if (k == 0 || k >= Q) continue;
                (uint256 x, uint256 y) = _affine(k);
                assertTrue(Pallas.isOnCurve(x, y));
                (uint256 xs, uint256 ys) = _affine(k == Q - 1 ? 1 : k + 1);
                (uint256 xa, uint256 ya, bool inf) = _add(x, y, Pallas.GX, Pallas.GY);
                if (k == Q - 1) {
                    assertTrue(inf);
                } else {
                    assertEq(xa, xs);
                    assertEq(ya, ys);
                }
            }
        }
    }

    /// Round-2 review (m7): boundary scalars. Powers of two exercise every doubling chain, Q - 2^i
    /// and the scalars just below Q set the top bits, and 2^254 +- 1 straddle Q's leading bit.
    /// Each is checked against the independent affine code: [k]G + [Q-k]G is the identity,
    /// [k]G + [k]G = [2k]G, and the canonical scalar (never its alias) opens the point.
    function test_boundaryScalars() public view {
        uint256[] memory ks = new uint256[](2 * 254 + 6);
        uint256 n;
        for (uint256 i = 0; i < 254; i++) {
            ks[n++] = uint256(1) << i;
            ks[n++] = Q - (uint256(1) << i);
        }
        ks[n++] = (uint256(1) << 254) - 1;
        ks[n++] = (uint256(1) << 254) + 1;
        ks[n++] = Q - 2;
        ks[n++] = Q - 3;
        ks[n++] = 3;
        ks[n++] = (Q - 1) / 2;
        for (uint256 j = 0; j < n; j++) {
            uint256 k = ks[j];
            (uint256 x, uint256 y) = _affine(k);
            assertTrue(Pallas.isOnCurve(x, y));
            assertTrue(Pallas.isDiscreteLogOf(k, x, y));
            assertFalse(Pallas.isDiscreteLogOf(k + Q, x, y));        // alias rejected
            assertTrue(Pallas.isDiscreteLogOf(Q - k, x, P - y));     // [Q-k]G = -[k]G
            (uint256 nx, uint256 ny) = _affine(Q - k);
            (,, bool inf) = _add(x, y, nx, ny);
            assertTrue(inf);                                         // independent check
            uint256 k2 = mulmod(k, 2, Q);
            if (k2 != 0) {
                (uint256 dx, uint256 dy, bool dinf) = _add(x, y, x, y);
                assertFalse(dinf);
                assertTrue(Pallas.isDiscreteLogOf(k2, dx, dy));      // doubling agrees
            }
        }
    }
}
