// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @notice VERIFICATION STUB (Halmos state-machine proofs only; never deployed).
/// isOnCurve / isDiscreteLogOf keep the real range guards but replace the curve arithmetic
/// with an uninterpreted predicate: the low bit of keccak256(tag, args). Halmos models keccak
/// as an uninterpreted (injective) function, so this bit is an arbitrary, deterministic
/// boolean function of the arguments. Any property proved against this stub therefore holds
/// for EVERY possible implementation of the curve check, including the real one.
library Pallas {
    uint256 internal constant P = 0x40000000000000000000000000000000224698fc094cf91b992d30ed00000001;
    uint256 internal constant Q = 0x40000000000000000000000000000000224698fc0994a8dd8c46eb2100000001;
    bytes32 internal constant TAG_CURVE = keccak256("stub.isOnCurve");
    bytes32 internal constant TAG_DLOG = keccak256("stub.isDiscreteLogOf");

    function isOnCurve(uint256 x, uint256 y) internal pure returns (bool) {
        if (x >= P || y >= P) return false;
        return uint256(keccak256(abi.encode(TAG_CURVE, x, y))) & 1 == 1;
    }

    function isDiscreteLogOf(uint256 k, uint256 x, uint256 y) internal pure returns (bool) {
        if (k == 0 || k >= Q) return false;
        if (x >= P || y >= P) return false; // mirrors the real guard (audit I-01 fix)
        return uint256(keccak256(abi.encode(TAG_DLOG, k, x, y))) & 1 == 1;
    }
}
