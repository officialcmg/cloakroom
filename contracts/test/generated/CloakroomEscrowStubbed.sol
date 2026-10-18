// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Pallas} from "./PallasStub.sol";

interface IERC20Esc {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function balanceOf(address who) external view returns (uint256);
}

/// @title CloakroomEscrow (v2)
/// @notice EVM leg of a Cloakroom swap: shielded ZEC (Zcash Ironwood) <-> ERC-20.
///
/// The ZEC holder ("taker") and the token holder ("maker") each hold one share of the spend
/// authorizing key of a shared Ironwood address: ask = a + b, A = [a]G, B = [b]G on Pallas.
/// The escrow holds the maker's tokens and has two exits, each of which publishes one share:
///   claim(a)  -> taker is paid; a is published, so the maker can sweep the ZEC with a + b;
///   refund(b) -> maker is refunded; b is published, so the taker can recover the ZEC.
///
/// v2 design rules (each closes a failure mode of v1):
///  * A claim requires Ready. The maker sets Ready only after seeing the ZEC note, so a
///    maker who is offline can be delayed but never loses tokens to an unfunded claim.
///  * A claim is two-phase. commitClaim locks the swap and publishes only H(id, a); the
///    taker reveals a in claim() only after the commitment is final. A commit that lands
///    too late simply reverts and leaks nothing.
///  * commitClaim must be authorized by the payout key (a key only the taker holds), and
///    the signature binds the relayer that earns the fee. Third parties cannot grief or
///    snipe the fee.
///  * Payments are pulled (withdraw). claim() and refund() never transfer tokens, so a
///    token-side failure (pause, blocklist) cannot make a share-revealing call revert.
///
///  * A punish window t2 (as in Gugger's XMR-BTC swap): from t2 the taker may lock even
///    without Ready, so her ZEC is never held hostage by a maker who disappears. A maker
///    must therefore come online once in [t1, t2) to refund an unfunded swap.
///
/// States and windows (t0 < t1 < t2; W = revealWindow):
///   Open   --setReady (maker, before t0)-->                         Ready
///   Ready  --commitClaim (payout-signed, before t1)-->              Locked
///   Open/Ready --commitClaim (payout-signed, from t2)-->            Locked   (punish)
///   Locked --claim(a) (before lockedAt + W)-->                      Claimed
///   Open   --refund(b) before t0-->                                 Refunded
///   Open/Ready --refund(b) in [t1, t2)-->                           Refunded
///   Locked --refund(b) from lockedAt + W-->                         Refunded
contract CloakroomEscrowStubbed {
    enum State {
        None,
        Open,
        Ready,
        Locked,
        Claimed,
        Refunded
    }

    struct Swap {
        address maker;
        address payout; // taker's fresh payout address; its key authorizes commitClaim
        address token;
        address relayer; // set at commitClaim, earns relayerFee
        uint64 t0;
        uint64 t1;
        uint64 t2;
        uint64 revealWindow;
        uint64 lockedAt;
        State state;
        uint256 amount;
        uint256 relayerFee;
        uint256 ax;
        uint256 ay;
        uint256 bx;
        uint256 by;
        bytes32 commitment;
    }

    mapping(bytes32 => Swap) public swaps;
    /// token => owner => withdrawable amount
    mapping(address => mapping(address => uint256)) public credit;

    event Opened(
        bytes32 indexed id,
        address indexed maker,
        address payout,
        address token,
        uint256 amount,
        uint256 relayerFee,
        uint256 ax,
        uint256 ay,
        uint256 bx,
        uint256 by,
        uint64 t0,
        uint64 t1,
        uint64 t2,
        uint64 revealWindow
    );
    event Ready(bytes32 indexed id);
    event Locked(bytes32 indexed id, bytes32 commitment, address relayer);
    event Claimed(bytes32 indexed id, uint256 a);
    event Refunded(bytes32 indexed id, uint256 b);
    event Withdrawn(address indexed token, address indexed owner, uint256 amount);

    error BadParams();
    error BadState();
    error TooEarly();
    error TooLate();
    error BadSecret();
    error BadSignature();
    error NotMaker();
    error NotClaimable();
    error UnsupportedToken();
    error TransferFailed();

    /// @notice Swap id: keccak256(abi.encode(maker, salt)), so ids are maker-scoped.
    function swapId(address maker, bytes32 salt) public pure returns (bytes32) {
        return keccak256(abi.encode(maker, salt));
    }

    /// @notice Digest the payout key signs (EIP-191 personal_sign over the inner hash).
    function commitDigest(bytes32 id, bytes32 commitment, address relayer) public view returns (bytes32) {
        bytes32 inner = keccak256(abi.encode(address(this), block.chainid, id, commitment, relayer));
        return keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", inner));
    }

    /// @notice The commitment a taker locks: keccak256(abi.encode(id, a)).
    function commitmentOf(bytes32 id, uint256 a) public pure returns (bytes32) {
        return keccak256(abi.encode(id, a));
    }

    function open(
        bytes32 salt,
        address payout,
        address token,
        uint256 amount,
        uint256 relayerFee,
        uint256 ax,
        uint256 ay,
        uint256 bx,
        uint256 by,
        uint64 t0,
        uint64 t1,
        uint64 t2,
        uint64 revealWindow
    ) external returns (bytes32 id) {
        if (
            payout == address(0) || amount == 0 || relayerFee > amount || t0 <= block.timestamp || t1 <= t0 || t2 <= t1
                || revealWindow == 0 || !Pallas.isOnCurve(ax, ay) || !Pallas.isOnCurve(bx, by)
        ) revert BadParams();
        id = swapId(msg.sender, salt);
        if (swaps[id].state != State.None) revert BadState();
        Swap storage s = swaps[id];
        s.maker = msg.sender;
        s.payout = payout;
        s.token = token;
        s.t0 = t0;
        s.t1 = t1;
        s.t2 = t2;
        s.revealWindow = revealWindow;
        s.state = State.Open;
        s.amount = amount;
        s.relayerFee = relayerFee;
        s.ax = ax;
        s.ay = ay;
        s.bx = bx;
        s.by = by;
        // Audit L-01: credit only what actually arrives. Fee-on-transfer, deflationary or
        // rebasing tokens would otherwise make the escrow under-collateralised.
        uint256 before = IERC20Esc(token).balanceOf(address(this));
        if (!IERC20Esc(token).transferFrom(msg.sender, address(this), amount)) revert TransferFailed();
        if (IERC20Esc(token).balanceOf(address(this)) - before != amount) revert UnsupportedToken();
        emit Opened(id, msg.sender, payout, token, amount, relayerFee, ax, ay, bx, by, t0, t1, t2, revealWindow);
    }

    /// @notice Maker confirms the ZEC note is at the shared address with enough confirmations.
    function setReady(bytes32 id) external {
        Swap storage s = swaps[id];
        if (msg.sender != s.maker) revert NotMaker();
        if (s.state != State.Open) revert BadState();
        if (block.timestamp >= s.t0) revert TooLate();
        s.state = State.Ready;
        emit Ready(id);
    }

    /// @notice Lock the swap for the taker. Publishes only H(id, a). Anyone may submit, but the
    ///         payout key must have signed (id, commitment, relayer). Allowed while Ready before
    ///         t1, or (punish path) in Open or Ready from t2.
    function commitClaim(bytes32 id, bytes32 commitment, address relayer, uint8 v, bytes32 r, bytes32 sg) external {
        Swap storage s = swaps[id];
        State st = s.state;
        if (st != State.Ready && st != State.Open) revert BadState();
        if (!((st == State.Ready && block.timestamp < s.t1) || block.timestamp >= s.t2)) revert NotClaimable();
        address signer = ecrecover(commitDigest(id, commitment, relayer), v, r, sg);
        if (signer == address(0) || signer != s.payout) revert BadSignature();
        s.state = State.Locked;
        s.commitment = commitment;
        s.relayer = relayer;
        s.lockedAt = uint64(block.timestamp);
        emit Locked(id, commitment, relayer);
    }

    /// @notice Reveal a. Credits the payout and the relayer; transfers happen in withdraw().
    function claim(bytes32 id, uint256 a) external {
        Swap storage s = swaps[id];
        if (s.state != State.Locked) revert BadState();
        if (block.timestamp >= uint256(s.lockedAt) + s.revealWindow) revert TooLate();
        if (commitmentOf(id, a) != s.commitment) revert BadSecret();
        if (!Pallas.isDiscreteLogOf(a, s.ax, s.ay)) revert BadSecret();
        s.state = State.Claimed;
        credit[s.token][s.payout] += s.amount - s.relayerFee;
        if (s.relayerFee != 0) credit[s.token][s.relayer] += s.relayerFee;
        emit Claimed(id, a);
    }

    /// @notice Reveal b and return the tokens to the maker (credited; see withdraw()).
    function refund(bytes32 id, uint256 b) external {
        Swap storage s = swaps[id];
        State st = s.state;
        bool ok = (st == State.Open && block.timestamp < s.t0)
            || ((st == State.Open || st == State.Ready) && block.timestamp >= s.t1 && block.timestamp < s.t2)
            || (st == State.Locked && block.timestamp >= uint256(s.lockedAt) + s.revealWindow);
        if (!ok) revert TooEarly();
        if (!Pallas.isDiscreteLogOf(b, s.bx, s.by)) revert BadSecret();
        s.state = State.Refunded;
        credit[s.token][s.maker] += s.amount;
        emit Refunded(id, b);
    }

    /// @notice Send `owner`'s credited balance of `token` to `owner`. Anyone may call, so a
    ///         payout address with no ETH can be paid by a relayer. Reveals nothing.
    function withdraw(address token, address owner) external {
        uint256 amt = credit[token][owner];
        credit[token][owner] = 0;
        if (amt != 0 && !IERC20Esc(token).transfer(owner, amt)) revert TransferFailed();
        emit Withdrawn(token, owner, amt);
    }
}
