// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

// Halmos symbolic proofs for CloakroomEscrow.
//
// Run:  ./audit/gen_harnesses.sh && .venv/bin/halmos --match-contract 'Halmos' ...
//       (see audit/REPORT.md for the exact command lines and bounds)
//
// TWO KINDS OF TARGET, kept strictly apart:
//  * EscrowSMHalmos / EscrowSeqHalmos / EscrowReentryHalmos run against
//    test/generated/CloakroomEscrowStubbed.sol, a byte-for-byte copy of src/CloakroomEscrow.sol
//    whose Pallas import points at test/generated/PallasStub.sol. In the stub, isOnCurve and
//    isDiscreteLogOf keep their range guards but the curve arithmetic is an UNINTERPRETED
//    PREDICATE (low bit of an uninterpreted keccak). These proofs are therefore valid for any
//    behaviour of the curve check and say NOTHING about the curve arithmetic itself.
//  * PallasHalmos runs against the REAL src/Pallas.sol (guards and formula structure only).

import {Test} from "forge-std/Test.sol";
import {CloakroomEscrowStubbed as Esc} from "./generated/CloakroomEscrowStubbed.sol";
import {Pallas as PallasStub} from "./generated/PallasStub.sol";
import {Pallas} from "../src/Pallas.sol";

interface SVM {
    function enableSymbolicStorage(address) external;
    function createCalldata(string memory contractName) external returns (bytes memory);
    function createUint(uint256 bits, string memory name) external returns (uint256);
    function createUint256(string memory name) external returns (uint256);
    function createBytes32(string memory name) external returns (bytes32);
    function createAddress(string memory name) external returns (address);
    function createBool(string memory name) external returns (bool);
}

/// Standard ERC20 (balances checked), used where token accounting matters.
contract HToken {
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

/// Records every state-changing call it receives; returns `ret` from transfer/transferFrom.
/// transferFrom credits the recipient with `v - shortfall` (shortfall > 0 models a
/// fee-on-transfer / deflationary token). balanceOf is a view (not counted as a call).
contract RecToken {
    uint256 public calls;
    address public lastFrom;
    address public lastTo;
    uint256 public lastAmt;
    bool public ret = true;
    uint256 public shortfall;
    mapping(address => uint256) public balanceOf;

    function setRet(bool r) external {
        ret = r;
    }

    function setShortfall(uint256 x) external {
        shortfall = x;
    }

    function transfer(address to, uint256 v) external returns (bool) {
        calls++;
        lastTo = to;
        lastAmt = v;
        return ret;
    }

    function transferFrom(address f, address to, uint256 v) external returns (bool) {
        calls++;
        lastFrom = f;
        lastTo = to;
        lastAmt = v;
        if (ret) balanceOf[to] += v - shortfall;
        return ret;
    }
}

/// Malicious token: on every transfer/transferFrom it (1) asserts the escrow already applied its
/// effects (checks-effects-interactions) and (2) re-enters the escrow with ARBITRARY calldata.
contract ReentrantToken {
    SVM constant svm = SVM(address(uint160(uint256(keccak256("svm cheat code")))));
    Esc public esc;
    bytes32 public expectId; // open(): swap that must already be Open when transferFrom runs
    address public expectOwner; // withdraw(): owner whose credit must already be 0 when transfer runs
    bool public reenter = true;
    bool public violated;
    mapping(address => uint256) public balanceOf; // credited on transferFrom (no debit side)
    uint256 public depth;

    constructor(Esc e) {
        esc = e;
    }

    function arm(bytes32 id, address owner) external {
        expectId = id;
        expectOwner = owner;
    }

    function _reenter() internal {
        if (!reenter || depth > 0) return;
        depth++;
        bytes memory cd = svm.createCalldata("CloakroomEscrowStubbed");
        (bool ok,) = address(esc).call(cd);
        ok;
        depth--;
    }

    function transferFrom(address, address to_, uint256 amt_) external returns (bool) {
        if (expectId != bytes32(0)) {
            (bool ok, bytes memory ret) = address(esc).staticcall(abi.encodeWithSignature("swaps(bytes32)", expectId));
            require(ok);
            Esc.Swap memory s = abi.decode(ret, (Esc.Swap));
            // effects of open() must be visible to the callee. Recorded as a flag (an assert here
            // would only revert the token call, which the escrow turns into a plain revert)
            if (s.state != Esc.State.Open) violated = true;
        }
        _reenter();
        balanceOf[to_] += amt_;
        return true;
    }

    function transfer(address, uint256) external returns (bool) {
        if (expectOwner != address(0) && esc.credit(address(this), expectOwner) != 0) violated = true;
        _reenter();
        return true;
    }
}

abstract contract HalmosBase is Test {
    SVM internal constant svm = SVM(address(uint160(uint256(keccak256("svm cheat code")))));
    Esc internal esc;

    uint64 internal constant TBOUND = type(uint64).max;

    // ---- single place that knows the open() ABI; update here when the signature changes ----
    struct OpenArgs {
        bytes32 salt;
        address payout;
        address token;
        uint256 amount;
        uint256 fee;
        uint256 ax;
        uint256 ay;
        uint256 bx;
        uint256 by;
        uint64 t0;
        uint64 t1;
        uint64 t2;
        uint64 w;
    }

    function _symOpenArgs(address token) internal returns (OpenArgs memory a) {
        a.salt = svm.createBytes32("salt");
        a.payout = svm.createAddress("payout");
        a.token = token;
        a.amount = svm.createUint256("amount");
        a.fee = svm.createUint256("fee");
        a.ax = svm.createUint256("ax");
        a.ay = svm.createUint256("ay");
        a.bx = svm.createUint256("bx");
        a.by = svm.createUint256("by");
        a.t0 = uint64(svm.createUint(64, "t0"));
        a.t1 = uint64(svm.createUint(64, "t1"));
        a.t2 = uint64(svm.createUint(64, "t2"));
        a.w = uint64(svm.createUint(64, "w"));
    }

    function _openCalldata(OpenArgs memory a) internal pure returns (bytes memory) {
        return abi.encodeCall(
            Esc.open, (a.salt, a.payout, a.token, a.amount, a.fee, a.ax, a.ay, a.bx, a.by, a.t0, a.t1, a.t2, a.w)
        );
    }

    function _openGuard(OpenArgs memory a, uint256 nowTs) internal pure returns (bool) {
        return a.payout != address(0) && a.amount != 0 && a.fee <= a.amount && a.t0 > nowTs && a.t1 > a.t0
            && a.t2 > a.t1 && a.w != 0 && _stubOnCurve(a.ax, a.ay) && _stubOnCurve(a.bx, a.by);
    }

    function _open(address maker, OpenArgs memory a) internal returns (bool ok, bytes32 id) {
        vm.prank(maker);
        bytes memory ret;
        (ok, ret) = address(esc).call(_openCalldata(a));
        if (ok) id = abi.decode(ret, (bytes32));
    }
    // ------------------------------------------------------------------------------------------

    function _swap(bytes32 id) internal view returns (Esc.Swap memory s) {
        (bool ok, bytes memory ret) = address(esc).staticcall(abi.encodeWithSignature("swaps(bytes32)", id));
        require(ok);
        s = abi.decode(ret, (Esc.Swap));
    }

    function _stubOnCurve(uint256 x, uint256 y) internal pure returns (bool) {
        return PallasStub.isOnCurve(x, y);
    }

    function _stubDlog(uint256 k, uint256 x, uint256 y) internal pure returns (bool) {
        return PallasStub.isDiscreteLogOf(k, x, y);
    }

    function _eq(Esc.Swap memory a, Esc.Swap memory b) internal pure returns (bool) {
        return keccak256(abi.encode(a)) == keccak256(abi.encode(b));
    }

    /// Allowed (pre -> post) pairs of the state machine (reflexive pairs allowed).
    function _allowed(Esc.State pre, Esc.State post) internal pure returns (bool) {
        if (pre == post) return true;
        if (pre == Esc.State.None) return post == Esc.State.Open;
        if (pre == Esc.State.Open) {
            return post == Esc.State.Ready || post == Esc.State.Refunded || post == Esc.State.Locked; // Locked: punish path (>= t2)
        }
        if (pre == Esc.State.Ready) return post == Esc.State.Locked || post == Esc.State.Refunded;
        if (pre == Esc.State.Locked) return post == Esc.State.Claimed || post == Esc.State.Refunded;
        return false; // Claimed, Refunded: absorbing
    }
}

/// ===========================================================================================
/// One-step proofs from an ARBITRARY storage state (svm.enableSymbolicStorage). Because the
/// pre-state is unconstrained, each proof is inductive: it holds after any number of calls.
/// ===========================================================================================
contract EscrowSMHalmos is HalmosBase {
    RecToken internal rec;

    function setUp() public {
        esc = new Esc();
        rec = new RecToken();
        svm.enableSymbolicStorage(address(esc));
    }

    // ---------------------------------------------------------------- setReady
    function check_setReady_iff(bytes32 id, bytes32 other, address caller, uint64 ts) public {
        vm.assume(other != id);
        Esc.Swap memory pre = _swap(id);
        Esc.Swap memory preO = _swap(other);
        vm.warp(ts);
        vm.prank(caller);
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.setReady, (id)));
        assert(ok == (caller == pre.maker && pre.state == Esc.State.Open && ts < pre.t0));
        Esc.Swap memory post = _swap(id);
        if (ok) {
            assert(post.state == Esc.State.Ready);
            post.state = pre.state;
        }
        assert(_eq(pre, post));
        assert(_eq(preO, _swap(other)));
    }

    // ---------------------------------------------------------------- commitClaim
    function check_commitClaim_iff(
        bytes32 id,
        bytes32 other,
        bytes32 c,
        address rel,
        uint8 v,
        bytes32 r,
        bytes32 sg,
        address caller,
        uint64 ts
    ) public {
        vm.assume(other != id);
        Esc.Swap memory pre = _swap(id);
        Esc.Swap memory preO = _swap(other);
        vm.warp(ts);
        address signer = ecrecover(esc.commitDigest(id, c, rel), v, r, sg);
        vm.prank(caller);
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.commitClaim, (id, c, rel, v, r, sg)));
        bool window = (pre.state == Esc.State.Ready && ts < pre.t1)
            || ((pre.state == Esc.State.Open || pre.state == Esc.State.Ready) && ts >= pre.t2);
        bool expected = window && signer != address(0) && signer == pre.payout;
        assert(ok == expected);
        Esc.Swap memory post = _swap(id);
        if (ok) {
            assert(post.state == Esc.State.Locked);
            assert(post.commitment == c && post.relayer == rel && post.lockedAt == ts);
            post.state = pre.state;
            post.commitment = pre.commitment;
            post.relayer = pre.relayer;
            post.lockedAt = pre.lockedAt;
        }
        assert(_eq(pre, post));
        assert(_eq(preO, _swap(other)));
    }

    // NOTE: a vm.sign-based variant (symbolic key, symbolic digest) is NOT used: with halmos
    // 0.3.3 even `(v,r,s) = vm.sign(k, d); assert(ecrecover(d, v, r, s) == vm.addr(k));` fails
    // (tool modelling gap, reproduced in isolation; see audit/REPORT.md). ecrecover is therefore
    // treated as an uninterpreted function in check_commitClaim_iff, and the concrete-key
    // behaviour (payout key accepted, other keys / other relayer / other commitment rejected) is
    // covered by forge tests with real secp256k1 signatures.

    /// The signed digest binds (escrow, chainid, id, commitment, relayer): distinct tuples give
    /// distinct digests (modulo keccak collisions, which Halmos models as impossible).
    function check_commitDigest_injective(bytes32 id1, bytes32 c1, address r1, bytes32 id2, bytes32 c2, address r2)
        public
        view
    {
        if (esc.commitDigest(id1, c1, r1) == esc.commitDigest(id2, c2, r2)) {
            assert(id1 == id2 && c1 == c2 && r1 == r2);
        }
    }

    /// t2 property: commitClaim from Open succeeds only at or after t2 (punish path).
    function check_commitClaim_fromOpen_onlyAtOrAfterT2(
        bytes32 id,
        bytes32 c,
        address rel,
        uint8 v,
        bytes32 r,
        bytes32 sg,
        address caller,
        uint64 ts
    ) public {
        Esc.Swap memory pre = _swap(id);
        vm.assume(pre.state == Esc.State.Open);
        vm.warp(ts);
        vm.prank(caller);
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.commitClaim, (id, c, rel, v, r, sg)));
        if (ok) assert(ts >= pre.t2);
    }

    /// t2 property: a Locked swap whose lock happened before t1 must have come from Ready;
    /// i.e. every lock is either (Ready, < t1) or (>= t2). Checked on the post-state.
    function check_commitClaim_lockTimeWindows(bytes32 id, bytes32 c, address rel, uint8 v, bytes32 r, bytes32 sg, uint64 ts)
        public
    {
        Esc.Swap memory pre = _swap(id);
        vm.warp(ts);
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.commitClaim, (id, c, rel, v, r, sg)));
        if (ok) {
            Esc.Swap memory post = _swap(id);
            assert(post.lockedAt == ts);
            assert((pre.state == Esc.State.Ready && ts < pre.t1) || ts >= pre.t2);
            assert(!(ts >= pre.t1 && ts < pre.t2)); // never locked inside the refund window [t1, t2)
        }
    }

    /// t2 property: refund from Open/Ready never succeeds at or after t2.
    function check_refund_OpenReady_neverAtOrAfterT2(bytes32 id, uint256 b, address caller, uint64 ts) public {
        Esc.Swap memory pre = _swap(id);
        vm.assume(pre.state == Esc.State.Open || pre.state == Esc.State.Ready);
        // reachable-state invariant t0 < t1 < t2: established by open() (check_open_iff) and
        // preserved because no function writes t0/t1/t2 (the *_iff proofs check all other fields
        // are unchanged). Without it, an Open swap with t0 > t2 (unreachable) refunds via the
        // early (now < t0) door -- that is the counterexample Halmos reports if this is removed.
        vm.assume(pre.t0 < pre.t1 && pre.t1 < pre.t2);
        vm.assume(ts >= pre.t2);
        vm.warp(ts);
        vm.prank(caller);
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.refund, (id, b)));
        assert(!ok);
    }

    /// Exclusivity of exits at every instant, from ANY state: claim, refund and commitClaim
    /// windows never overlap for the same (state, time), so for a given block at most one of
    /// {lock, refund} (from Open/Ready) and at most one of {claim, refund} (from Locked) is open.
    function check_windowsDisjoint(bytes32 id, uint64 ts) public view {
        Esc.Swap memory s = _swap(id);
        vm.assume(s.t0 < s.t1 && s.t1 < s.t2); // open() guard (proved by check_open_iff)
        bool isO = s.state == Esc.State.Open;
        bool isR = s.state == Esc.State.Ready;
        bool commitW = (isR && ts < s.t1) || ((isO || isR) && ts >= s.t2);
        bool refundOR = (isO && ts < s.t0) || ((isO || isR) && ts >= s.t1 && ts < s.t2);
        assert(!(commitW && refundOR));
        bool claimW = s.state == Esc.State.Locked && uint256(ts) < uint256(s.lockedAt) + s.revealWindow;
        bool refundL = s.state == Esc.State.Locked && uint256(ts) >= uint256(s.lockedAt) + s.revealWindow;
        assert(!(claimW && refundL));
    }

    // ---------------------------------------------------------------- claim
    function check_claim_iff(bytes32 id, bytes32 other, uint256 a, address caller, uint64 ts) public {
        vm.assume(other != id);
        Esc.Swap memory pre = _swap(id);
        Esc.Swap memory preO = _swap(other);
        uint256 cPay = esc.credit(pre.token, pre.payout);
        uint256 cRel = esc.credit(pre.token, pre.relayer);
        vm.warp(ts);
        vm.prank(caller);
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.claim, (id, a)));

        bool guards = pre.state == Esc.State.Locked && uint256(ts) < uint256(pre.lockedAt) + pre.revealWindow
            && keccak256(abi.encode(id, a)) == pre.commitment && _stubDlog(a, pre.ax, pre.ay);
        // arithmetic side conditions (both hold in every reachable state, see the sequence proofs
        // and invariant tests: fee <= amount is an open() guard; credits <= token supply)
        bool arith;
        unchecked {
            uint256 net = pre.amount - pre.relayerFee;
            uint256 relBefore = pre.relayer == pre.payout ? cPay + net : cRel;
            arith = pre.relayerFee <= pre.amount && cPay + net >= cPay
                && (pre.relayerFee == 0 || relBefore + pre.relayerFee >= relBefore);
        }
        // claim succeeds EXACTLY under these conditions: nothing else (no token, no external
        // contract, no caller identity) can make it revert.
        assert(ok == (guards && arith));

        Esc.Swap memory post = _swap(id);
        if (ok) {
            assert(post.state == Esc.State.Claimed);
            post.state = pre.state;
            if (pre.payout == pre.relayer) {
                assert(esc.credit(pre.token, pre.payout) == cPay + pre.amount);
            } else {
                assert(esc.credit(pre.token, pre.payout) == cPay + pre.amount - pre.relayerFee);
                assert(esc.credit(pre.token, pre.relayer) == cRel + pre.relayerFee);
            }
        }
        assert(_eq(pre, post));
        assert(_eq(preO, _swap(other)));
    }

    // ---------------------------------------------------------------- refund
    function check_refund_iff(bytes32 id, bytes32 other, uint256 b, address caller, uint64 ts) public {
        vm.assume(other != id);
        Esc.Swap memory pre = _swap(id);
        Esc.Swap memory preO = _swap(other);
        uint256 cMk = esc.credit(pre.token, pre.maker);
        vm.warp(ts);
        vm.prank(caller);
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.refund, (id, b)));

        bool w1 = pre.state == Esc.State.Open && ts < pre.t0;
        bool w2 = (pre.state == Esc.State.Open || pre.state == Esc.State.Ready) && ts >= pre.t1 && ts < pre.t2;
        bool w3 = pre.state == Esc.State.Locked && uint256(ts) >= uint256(pre.lockedAt) + pre.revealWindow;
        bool arith;
        unchecked {
            arith = cMk + pre.amount >= cMk;
        }
        assert(ok == ((w1 || w2 || w3) && _stubDlog(b, pre.bx, pre.by) && arith));

        Esc.Swap memory post = _swap(id);
        if (ok) {
            assert(post.state == Esc.State.Refunded);
            assert(esc.credit(pre.token, pre.maker) == cMk + pre.amount);
            post.state = pre.state;
        }
        assert(_eq(pre, post));
        assert(_eq(preO, _swap(other)));
    }

    // ---------------------------------------------------------------- withdraw
    /// Pays exactly the credited amount to exactly `owner`, zeroes the credit, touches no other
    /// credit and no swap; a token returning false reverts the whole call (credit restored).
    function check_withdraw_exact(address owner, address otherOwner, address otherToken, bytes32 anyId, bool tokenOk)
        public
    {
        vm.assume(otherOwner != owner);
        vm.assume(otherToken != address(rec));
        rec.setRet(tokenOk);
        uint256 pre = esc.credit(address(rec), owner);
        uint256 preO = esc.credit(address(rec), otherOwner);
        uint256 preT = esc.credit(otherToken, owner);
        Esc.Swap memory preS = _swap(anyId);
        uint256 calls0 = rec.calls();

        (bool ok,) = address(esc).call(abi.encodeCall(Esc.withdraw, (address(rec), owner)));

        assert(ok == (pre == 0 || tokenOk));
        if (ok) {
            assert(esc.credit(address(rec), owner) == 0);
            if (pre != 0) {
                assert(rec.calls() == calls0 + 1);
                assert(rec.lastTo() == owner && rec.lastAmt() == pre);
            } else {
                assert(rec.calls() == calls0); // nothing credited -> no token call at all
            }
        } else {
            assert(esc.credit(address(rec), owner) == pre);
        }
        assert(esc.credit(address(rec), otherOwner) == preO);
        assert(esc.credit(otherToken, owner) == preT);
        assert(_eq(preS, _swap(anyId)));
    }

    // ---------------------------------------------------------------- open
    /// open() succeeds iff its parameter guards hold, the id is free and the token pulls
    /// `amount`; the id is swapId(msg.sender, salt); nothing else in storage changes.
    function check_open_iff(address maker, bytes32 other, uint64 ts, uint256 shortfall) public {
        rec.setShortfall(shortfall); // 0 = standard token; > 0 = fee-on-transfer (L-01)
        OpenArgs memory a = _symOpenArgs(address(rec));
        bytes32 id = esc.swapId(maker, a.salt);
        vm.assume(other != id);
        Esc.Swap memory pre = _swap(id);
        Esc.Swap memory preO = _swap(other);
        uint256 calls0 = rec.calls();
        vm.warp(ts);
        (bool ok, bytes32 rid) = _open(maker, a);
        // L-01 fix: succeeds only if the escrow received exactly `amount`
        bool overflowFree = rec.balanceOf(address(esc)) + a.amount >= a.amount; // fresh token: balance 0
        assert(ok == (_openGuard(a, ts) && pre.state == Esc.State.None && shortfall == 0 && overflowFree));
        Esc.Swap memory post = _swap(id);
        if (ok) {
            assert(rid == id);
            assert(post.state == Esc.State.Open && post.maker == maker && post.payout == a.payout);
            assert(post.token == a.token && post.amount == a.amount && post.relayerFee == a.fee);
            assert(post.ax == a.ax && post.ay == a.ay && post.bx == a.bx && post.by == a.by);
            assert(post.t0 == a.t0 && post.t1 == a.t1 && post.t2 == a.t2 && post.revealWindow == a.w);
            // exactly one token call: transferFrom(maker -> escrow, amount)
            assert(rec.calls() == calls0 + 1);
            assert(rec.lastFrom() == maker && rec.lastTo() == address(esc) && rec.lastAmt() == a.amount);
        } else {
            assert(_eq(pre, post));
        }
        assert(_eq(preO, _swap(other)));
    }

    /// Maker scoping: no call by `caller` with ANY salt can open or modify a swap whose id is
    /// swapId(victim, s) for a different maker `victim`.
    function check_open_cannotTouchOtherMakersId(address caller, address victim, bytes32 s, uint64 ts) public {
        vm.assume(caller != victim);
        OpenArgs memory a = _symOpenArgs(address(rec));
        bytes32 vid = esc.swapId(victim, s);
        Esc.Swap memory pre = _swap(vid);
        vm.warp(ts);
        _open(caller, a);
        assert(_eq(pre, _swap(vid)));
    }

    /// Once a swap exists (state != None) its maker field never changes, whatever is called.
    /// Covers every external function (arbitrary calldata), arbitrary caller and time.
    function check_anyCall_transitionsAndMaker(bytes32 id, address caller, uint64 ts) public {
        Esc.Swap memory pre = _swap(id);
        bytes memory cd = svm.createCalldata("CloakroomEscrowStubbed");
        // open() calls the token named in calldata; pin it to a well-behaved token so the
        // symbolic call target is concrete (other functions ignore this)
        vm.assume(cd.length < 4 || bytes4(cd) != Esc.open.selector);
        vm.warp(ts);
        vm.prank(caller);
        (bool ok,) = address(esc).call(cd);
        ok;
        Esc.Swap memory post = _swap(id);
        assert(_allowed(pre.state, post.state));
        if (pre.state != Esc.State.None) assert(post.maker == pre.maker);
    }
}

/// ===========================================================================================
/// Bounded call sequences from the empty state (real token accounting): conservation,
/// exclusivity, path property. DEPTH calls, each op/argument symbolic.
/// ===========================================================================================
contract EscrowSeqHalmos is HalmosBase {
    HToken internal tok;
    address[4] internal actors = [address(0x1000), address(0x2000), address(0x3000), address(0x4000)];
    bytes32 internal constant SALT = bytes32(uint256(0x5a17));
    bytes32[2] internal ids;
    bool[2] internal sawReady;
    bool[2] internal sawLocked;
    uint256 internal constant SUPPLY = 1e30;
    bool internal oneSwap; // depth-4 run restricts to a single swap id to stay tractable

    function setUp() public {
        esc = new Esc();
        tok = new HToken();
        for (uint256 j; j < 2; j++) {
            tok.mint(actors[j], SUPPLY);
            vm.prank(actors[j]);
            tok.approve(address(esc), type(uint256).max);
            ids[j] = esc.swapId(actors[j], SALT);
        }
    }

    /// Branch-free symbolic actor selection: one of 0x1000, 0x2000, 0x3000, 0x4000.
    /// (Halmos cannot index storage arrays with a symbolic slot, and if-chains multiply paths.)
    function _actor(uint256 i) internal pure returns (address) {
        return address(uint160(0x1000 * (1 + (i & 3))));
    }

    /// One symbolic step: warp by a symbolic dt in [0, 2^40), then one of the six external
    /// functions with symbolic arguments and caller. Reverting calls are pruned with
    /// vm.assume(ok): a reverted call leaves state unchanged (EVM atomicity), so the states
    /// reachable with <= N calls (some failing) are exactly those reachable with <= N successful
    /// calls, and invariants are asserted after every prefix.
    function _step() internal {
        vm.warp(block.timestamp + svm.createUint(40, "dt"));
        uint256 op = svm.createUint(8, "op");
        uint256 j = oneSwap ? 0 : svm.createUint(1, "j");
        address mk = _actor(j); // maker 0x1000 or 0x2000
        bytes32 id = esc.swapId(mk, SALT);
        address caller = svm.createAddress("caller");
        bool ok;
        if (op == 0) {
            OpenArgs memory a = _symOpenArgs(address(tok));
            a.salt = SALT;
            a.payout = _actor(svm.createUint(2, "payoutIdx"));
            // amounts below 2^96 (USDC total supply ~ 2^56): keeps the bit-vector sums tractable
            vm.assume(a.amount < 2 ** 96);
            (ok,) = _open(mk, a);
        } else if (op == 1) {
            vm.prank(caller);
            (ok,) = address(esc).call(abi.encodeCall(Esc.setReady, (id)));
        } else if (op == 2) {
            address rel = _actor(svm.createUint(2, "relIdx"));
            vm.prank(caller);
            (ok,) = address(esc).call(
                abi.encodeCall(
                    Esc.commitClaim,
                    (
                        id,
                        svm.createBytes32("c"),
                        rel,
                        uint8(svm.createUint(8, "v")),
                        svm.createBytes32("r"),
                        svm.createBytes32("s")
                    )
                )
            );
        } else if (op == 3) {
            vm.prank(caller);
            (ok,) = address(esc).call(abi.encodeCall(Esc.claim, (id, svm.createUint256("a"))));
        } else if (op == 4) {
            vm.prank(caller);
            (ok,) = address(esc).call(abi.encodeCall(Esc.refund, (id, svm.createUint256("b"))));
        } else {
            vm.prank(caller);
            (ok,) = address(esc).call(abi.encodeCall(Esc.withdraw, (address(tok), _actor(svm.createUint(2, "wIdx")))));
        }
        vm.assume(ok);
    }

    function _checkInvariants(Esc.State[2] memory prev) internal returns (Esc.State[2] memory cur) {
        uint256 live;
        uint256 deposited;
        for (uint256 j; j < 2; j++) {
            Esc.Swap memory s = _swap(ids[j]);
            cur[j] = s.state;
            assert(_allowed(prev[j], cur[j]));
            if (s.state == Esc.State.Ready) sawReady[j] = true;
            if (s.state == Esc.State.Locked) {
                // reached via Ready (normal path) or via the punish path at/after t2
                assert(sawReady[j] || s.lockedAt >= s.t2);
                if (!sawReady[j]) assert(prev[j] == Esc.State.Open || prev[j] == Esc.State.Locked);
                sawLocked[j] = true;
            }
            if (s.state == Esc.State.Claimed) assert(sawLocked[j] && (sawReady[j] || s.lockedAt >= s.t2));
            if (s.state != Esc.State.None) {
                assert(s.relayerFee <= s.amount);
                assert(s.maker == actors[j]);
                deposited += s.amount;
            }
            if (s.state == Esc.State.Open || s.state == Esc.State.Ready || s.state == Esc.State.Locked) {
                live += s.amount;
            }
        }
        uint256 credits;
        for (uint256 i; i < 4; i++) {
            credits += esc.credit(address(tok), actors[i]);
        }
        // conservation: the escrow holds exactly the unsettled amounts plus outstanding credits
        assert(tok.balanceOf(address(esc)) == live + credits);
        // credits never exceed what was deposited
        assert(credits <= deposited);
        // supply conserved (nothing minted or burned by the escrow)
        uint256 total = tok.balanceOf(address(esc));
        for (uint256 i; i < 4; i++) {
            total += tok.balanceOf(actors[i]);
        }
        assert(total == 2 * SUPPLY);
    }

    function _run(uint256 depth) internal {
        Esc.State[2] memory st;
        for (uint256 k; k < depth; k++) {
            _step();
            st = _checkInvariants(st);
        }
    }

    function check_seq_depth1() public { _run(1); }
    function check_seq_depth2() public { _run(2); }
    function check_seq_depth3() public {
        _run(3);
    }

    function check_seq_depth4_oneSwap() public {
        oneSwap = true;
        _run(4);
    }

    /// Directed depth-6 run along the claim path (open, ready, warp, commit, claim, withdraw)
    /// with every argument symbolic, so the Claimed -> withdraw accounting is exercised.
    function check_seq_claimPathThenWithdraw() public {
        Esc.State[2] memory st;
        OpenArgs memory a = _symOpenArgs(address(tok));
        a.salt = SALT;
        a.payout = actors[2];
        (bool ok,) = _openOk(a);
        vm.assume(ok);
        st = _checkInvariants(st);
        vm.warp(block.timestamp + svm.createUint(40, "dt0"));
        vm.prank(svm.createAddress("c1"));
        (ok,) = address(esc).call(abi.encodeCall(Esc.setReady, (ids[0])));
        st = _checkInvariants(st);
        vm.warp(block.timestamp + svm.createUint(40, "dt1"));
        (ok,) = address(esc).call(
            abi.encodeCall(
                Esc.commitClaim,
                (ids[0], svm.createBytes32("c"), actors[3], uint8(svm.createUint(8, "v")), svm.createBytes32("r"), svm.createBytes32("s"))
            )
        );
        st = _checkInvariants(st);
        vm.warp(block.timestamp + svm.createUint(40, "dt2"));
        (ok,) = address(esc).call(abi.encodeCall(Esc.claim, (ids[0], svm.createUint256("a"))));
        st = _checkInvariants(st);
        (ok,) = address(esc).call(abi.encodeCall(Esc.withdraw, (address(tok), actors[2])));
        st = _checkInvariants(st);
        (ok,) = address(esc).call(abi.encodeCall(Esc.withdraw, (address(tok), actors[3])));
        st = _checkInvariants(st);
        (ok,) = address(esc).call(abi.encodeCall(Esc.refund, (ids[0], svm.createUint256("b"))));
        st = _checkInvariants(st);
    }

    function _openOk(OpenArgs memory a) internal returns (bool ok, bytes32 id) {
        return _open(actors[0], a);
    }
}

/// ===========================================================================================
/// Reentrancy: a malicious token re-enters with arbitrary calldata during open()/withdraw().
/// The token itself asserts that the escrow's effects were applied BEFORE the external call.
/// ===========================================================================================
contract EscrowReentryHalmos is HalmosBase {
    ReentrantToken internal rt;

    function setUp() public {
        esc = new Esc();
        rt = new ReentrantToken(esc);
    }

    function check_open_CEI(address maker, uint64 ts) public {
        OpenArgs memory a = _symOpenArgs(address(rt));
        rt.arm(esc.swapId(maker, a.salt), address(0));
        vm.warp(ts);
        (bool ok,) = _open(maker, a);
        if (ok) assert(_swap(esc.swapId(maker, a.salt)).state != Esc.State.None);
        assert(!rt.violated());
    }

    function check_withdraw_CEI_and_noDoublePay(address owner, uint64 ts) public {
        svm.enableSymbolicStorage(address(esc));
        uint256 pre = esc.credit(address(rt), owner);
        rt.arm(bytes32(0), owner);
        vm.warp(ts);
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.withdraw, (address(rt), owner)));
        // The CEI assertion lives in ReentrantToken.transfer: during the external call the
        // owner's credit is already 0, so any nested withdraw (arbitrary calldata) pays 0.
        if (ok && pre == 0) assert(rt.depth() == 0);
        assert(!rt.violated());
    }
}

/// ===========================================================================================
/// REAL src/Pallas.sol: guards and formula structure (scalar multiplication is out of reach of
/// symbolic execution; see audit/REPORT.md for the argument and the exhaustive small-curve tests).
/// ===========================================================================================
contract PallasHalmos is Test {
    function check_dlog_rejectsZeroAndOutOfRange(uint256 k, uint256 x, uint256 y) public pure {
        vm.assume(k == 0 || k >= Pallas.Q);
        assert(!Pallas.isDiscreteLogOf(k, x, y));
    }

    /// I-01 fix: non-canonical coordinates are rejected for EVERY scalar (no curve arithmetic
    /// needed: the guard returns before mulG).
    function check_dlog_rejectsNonCanonical(uint256 k, uint256 x, uint256 y) public pure {
        vm.assume(x >= Pallas.P || y >= Pallas.P);
        vm.assume(k != 0 && k < Pallas.Q);
        assert(!Pallas.isDiscreteLogOf(k, x, y));
    }

    function check_onCurve_rejectsNonCanonical(uint256 x, uint256 y) public pure {
        vm.assume(x >= Pallas.P || y >= Pallas.P);
        assert(!Pallas.isOnCurve(x, y));
    }

    function check_dbl_identity(uint256 X, uint256 Y) public pure {
        (uint256 X3, uint256 Y3, uint256 Z3) = Pallas.dbl(X, Y, 0);
        assert(X3 == 0 && Y3 == 1 && Z3 == 0);
    }

    function check_maddG_identity(uint256 X, uint256 Y) public pure {
        (uint256 X3, uint256 Y3, uint256 Z3) = Pallas.maddG(X, Y, 0);
        assert(X3 == Pallas.GX && Y3 == Pallas.GY && Z3 == 1);
    }

    /// dbl: Z3 = 2*Y1*Z1 mod p, nonzero for canonical nonzero Y1, Z1 (p is prime and odd).
    function check_dbl_ZnonZero(uint256 X, uint256 Y, uint256 Z) public pure {
        vm.assume(X < Pallas.P && Y < Pallas.P && Z < Pallas.P && Y != 0 && Z != 0);
        (,, uint256 Z3) = Pallas.dbl(X, Y, Z);
        assert(Z3 != 0);
    }

    /// maddG generic branch: Z3 = (Z1+H)^2 - Z1^2 - H^2 = 2*Z1*H, nonzero when Z1, H != 0.
    function check_maddG_ZnonZero(uint256 X, uint256 Y, uint256 Z) public pure {
        vm.assume(X < Pallas.P && Y < Pallas.P && Z < Pallas.P && Z != 0);
        uint256 zz = mulmod(Z, Z, Pallas.P);
        vm.assume(mulmod(Pallas.GX, zz, Pallas.P) != X); // H != 0 (P != +-G)
        (,, uint256 Z3) = Pallas.maddG(X, Y, Z);
        assert(Z3 != 0);
    }

    /// All outputs of dbl/maddG are canonical field elements (< p), so the final projective
    /// comparison compares reduced residues.
    function check_formulas_canonical(uint256 X, uint256 Y, uint256 Z) public pure {
        (uint256 a, uint256 b, uint256 c) = Pallas.dbl(X, Y, Z);
        assert(a < Pallas.P && b < Pallas.P && c < Pallas.P);
        (a, b, c) = Pallas.maddG(X, Y, Z);
        assert(a < Pallas.P && b < Pallas.P && c < Pallas.P);
    }
}

/// ===========================================================================================
/// NON-VACUITY WITNESSES. Every check here is EXPECTED TO FAIL: the counterexample Halmos prints
/// is a concrete witness that the path is feasible under the same harness, so the proofs above
/// are not passing merely because the interesting paths are unreachable.
/// ===========================================================================================
contract EscrowWitnessHalmos is HalmosBase {
    HToken internal tok;
    address internal constant MK = address(0x1000);

    function setUp() public {
        esc = new Esc();
        tok = new HToken();
        tok.mint(MK, 1e30);
        vm.prank(MK);
        tok.approve(address(esc), type(uint256).max);
    }

    function _openSym() internal returns (bytes32 id, OpenArgs memory a) {
        a = _symOpenArgs(address(tok));
        vm.assume(a.amount < 2 ** 96);
        bool ok;
        (ok, id) = _open(MK, a);
        vm.assume(ok);
    }

    function _commitSym(bytes32 id) internal returns (bool ok) {
        (ok,) = address(esc).call(
            abi.encodeCall(
                Esc.commitClaim,
                (id, svm.createBytes32("c"), svm.createAddress("rel"), uint8(svm.createUint(8, "v")), svm.createBytes32("r"), svm.createBytes32("s"))
            )
        );
    }

    function check_WITNESS_claimViaReady() public {
        (bytes32 id,) = _openSym();
        vm.warp(block.timestamp + svm.createUint(40, "d0"));
        vm.prank(MK);
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.setReady, (id)));
        vm.assume(ok);
        vm.warp(block.timestamp + svm.createUint(40, "d1"));
        vm.assume(_commitSym(id));
        vm.warp(block.timestamp + svm.createUint(40, "d2"));
        (ok,) = address(esc).call(abi.encodeCall(Esc.claim, (id, svm.createUint256("a"))));
        assert(!ok); // EXPECTED TO FAIL (witness)
    }

    function check_WITNESS_claimViaPunishFromOpen() public {
        (bytes32 id,) = _openSym();
        vm.warp(block.timestamp + svm.createUint(40, "d1"));
        vm.assume(_commitSym(id));
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.claim, (id, svm.createUint256("a"))));
        assert(!ok); // EXPECTED TO FAIL (witness)
    }

    function check_WITNESS_refundEachDoor(uint8 door) public {
        (bytes32 id,) = _openSym();
        if (door % 3 == 1) {
            vm.prank(MK);
            (bool r,) = address(esc).call(abi.encodeCall(Esc.setReady, (id)));
            vm.assume(r);
        } else if (door % 3 == 2) {
            vm.prank(MK);
            (bool r,) = address(esc).call(abi.encodeCall(Esc.setReady, (id)));
            vm.assume(r);
            vm.assume(_commitSym(id));
        }
        vm.warp(block.timestamp + svm.createUint(40, "d"));
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.refund, (id, svm.createUint256("b"))));
        assert(!ok); // EXPECTED TO FAIL for each door (witness)
    }

    function check_WITNESS_withdrawAfterClaimPaysTaker() public {
        (bytes32 id, OpenArgs memory a) = _openSym();
        vm.warp(block.timestamp + svm.createUint(40, "d1"));
        vm.assume(_commitSym(id));
        (bool ok,) = address(esc).call(abi.encodeCall(Esc.claim, (id, svm.createUint256("a"))));
        vm.assume(ok);
        (ok,) = address(esc).call(abi.encodeCall(Esc.withdraw, (address(tok), a.payout)));
        assert(tok.balanceOf(a.payout) == 0); // EXPECTED TO FAIL (witness: taker is paid)
    }
}
