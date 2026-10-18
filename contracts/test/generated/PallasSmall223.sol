// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title Pallas
/// @notice Minimal arithmetic on the Pallas curve (y^2 = x^3 + 5 over F_p) for checking
///         that a revealed scalar is the discrete log of a stored point with respect to
///         G^Orchard, the RedPallas SpendAuth basepoint used by Zcash Orchard/Ironwood.
/// @dev Jacobian coordinates, a = 0 formulas (dbl-2009-l, madd-2007-bl). The final
///      comparison is done projectively, so no field inversion is needed.
library PallasSmall223 {
    uint256 internal constant P = 223;
    uint256 internal constant Q = 229;
    uint256 internal constant B = 5;

    // G^Orchard = GroupHash^P("z.cash:Orchard", "G"), extracted from the orchard 0.16 crate.
    uint256 internal constant GX = 3;
    uint256 internal constant GY = 60;

    /// @notice True iff (x, y) is an affine point on Pallas with canonical coordinates.
    function isOnCurve(uint256 x, uint256 y) internal pure returns (bool) {
        if (x >= P || y >= P) return false;
        uint256 lhs = mulmod(y, y, P);
        uint256 rhs = addmod(mulmod(mulmod(x, x, P), x, P), B, P);
        return lhs == rhs;
    }

    /// @notice True iff 0 < k < Q and [k]G == (x, y).
    function isDiscreteLogOf(uint256 k, uint256 x, uint256 y) internal pure returns (bool) {
        if (k == 0 || k >= Q) return false;
        // Audit I-01: require canonical coordinates here too, not only in callers.
        if (x >= P || y >= P) return false;
        (uint256 X, uint256 Y, uint256 Z) = mulG(k);
        if (Z == 0) return false;
        uint256 zz = mulmod(Z, Z, P);
        // Affine (x, y) equals Jacobian (X, Y, Z) iff X == x*Z^2 and Y == y*Z^3.
        return X == mulmod(x, zz, P) && Y == mulmod(y, mulmod(zz, Z, P), P);
    }

    /// @notice [k]G in Jacobian coordinates, MSB-first double-and-add.
    function mulG(uint256 k) internal pure returns (uint256 X, uint256 Y, uint256 Z) {
        unchecked {
            // Q < 2^255, so bit 254 is the highest bit that can be set.
            for (uint256 i = 255; i > 0; --i) {
                (X, Y, Z) = dbl(X, Y, Z);
                if ((k >> (i - 1)) & 1 == 1) {
                    (X, Y, Z) = maddG(X, Y, Z);
                }
            }
        }
    }

    function dbl(uint256 X1, uint256 Y1, uint256 Z1) internal pure returns (uint256 X3, uint256 Y3, uint256 Z3) {
        if (Z1 == 0) return (0, 1, 0);
        uint256 p = P;
        uint256 a = mulmod(X1, X1, p);
        uint256 b = mulmod(Y1, Y1, p);
        uint256 c = mulmod(b, b, p);
        uint256 t = addmod(X1, b, p);
        uint256 d = mulmod(t, t, p);
        d = addmod(d, p - addmod(a, c, p), p);
        d = addmod(d, d, p);
        uint256 e = mulmod(3, a, p);
        uint256 f = mulmod(e, e, p);
        X3 = addmod(f, p - addmod(d, d, p), p);
        Y3 = addmod(mulmod(e, addmod(d, p - X3, p), p), p - mulmod(8, c, p), p);
        Z3 = mulmod(addmod(Y1, Y1, p), Z1, p);
    }

    /// @dev (X1:Y1:Z1) + G, with G affine (Z2 = 1).
    function maddG(uint256 X1, uint256 Y1, uint256 Z1) internal pure returns (uint256 X3, uint256 Y3, uint256 Z3) {
        if (Z1 == 0) return (GX, GY, 1);
        uint256 p = P;
        uint256 z1z1 = mulmod(Z1, Z1, p);
        uint256 u2 = mulmod(GX, z1z1, p);
        uint256 s2 = mulmod(GY, mulmod(Z1, z1z1, p), p);
        uint256 h = addmod(u2, p - X1, p);
        uint256 r = addmod(s2, p - Y1, p);
        if (h == 0) {
            if (r == 0) return dbl(X1, Y1, Z1); // P == G
            return (0, 1, 0); // P == -G
        }
        r = addmod(r, r, p);
        uint256 hh = mulmod(h, h, p);
        uint256 i = mulmod(4, hh, p);
        uint256 j = mulmod(h, i, p);
        uint256 v = mulmod(X1, i, p);
        X3 = addmod(addmod(mulmod(r, r, p), p - j, p), p - addmod(v, v, p), p);
        uint256 y1j = mulmod(Y1, j, p);
        Y3 = addmod(mulmod(r, addmod(v, p - X3, p), p), p - addmod(y1j, y1j, p), p);
        uint256 zh = addmod(Z1, h, p);
        Z3 = addmod(mulmod(zh, zh, p), p - addmod(z1z1, hh, p), p);
    }
}
