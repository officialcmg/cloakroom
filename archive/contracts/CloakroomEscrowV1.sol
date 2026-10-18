// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Pallas} from "../src/Pallas.sol";

interface IERC20 {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
}

/// @title CloakroomEscrow
/// @notice EVM leg of a Cloakroom swap: shielded ZEC (Zcash Ironwood) <-> ERC-20.
///
/// The ZEC holder ("taker") and the token holder ("maker") each hold one share of the
/// spend authorizing key of a shared Ironwood address: ask = a + b, with A = [a]G and
/// B = [b]G on Pallas. This contract holds the maker's tokens. It has two exits:
///   claim(a)  pays the taker and publishes a, so the maker can spend the ZEC with a + b;
///   refund(b) pays the maker and publishes b, so the taker can recover the ZEC with a + b.
/// Exactly one exit can ever succeed.
///
/// Timeline (t0 < t1):
///   [open, t0)   maker may setReady() after verifying the ZEC note; maker may refund
///                only while NOT ready; taker may claim only once ready.
///   [t0, t1)     taker may claim (ready or not); no refunds.
///   [t1, inf)    maker may refund; no claims.
contract CloakroomEscrowV1 {
    enum State {
        None,
        Open,
        Ready,
        Claimed,
        Refunded
    }

    struct Swap {
        address maker;
        address payout; // taker's payout address, fixed at open
        address token;
        uint64 t0;
        uint64 t1;
        State state;
        uint256 amount;
        uint256 relayerFee; // paid to whoever submits claim(), enabling gasless claims
        uint256 ax;
        uint256 ay;
        uint256 bx;
        uint256 by;
    }

    mapping(bytes32 => Swap) public swaps;

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
        uint64 t1
    );
    event Ready(bytes32 indexed id);
    event Claimed(bytes32 indexed id, uint256 a);
    event Refunded(bytes32 indexed id, uint256 b);

    error BadParams();
    error BadState();
    error TooEarly();
    error TooLate();
    error BadSecret();
    error NotMaker();
    error TransferFailed();

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
        uint64 t1
    ) external returns (bytes32 id) {
        if (
            payout == address(0) || amount == 0 || relayerFee > amount || t0 <= block.timestamp || t1 <= t0
                || !Pallas.isOnCurve(ax, ay) || !Pallas.isOnCurve(bx, by)
        ) revert BadParams();
        id = keccak256(abi.encode(msg.sender, salt));
        if (swaps[id].state != State.None) revert BadState();
        swaps[id] = Swap(msg.sender, payout, token, t0, t1, State.Open, amount, relayerFee, ax, ay, bx, by);
        if (!IERC20(token).transferFrom(msg.sender, address(this), amount)) revert TransferFailed();
        emit Opened(id, msg.sender, payout, token, amount, relayerFee, ax, ay, bx, by, t0, t1);
    }

    /// @notice Maker confirms the shielded ZEC note is in the shared address (checked off-chain
    ///         with the shared full viewing key) with sufficient confirmations.
    function setReady(bytes32 id) external {
        Swap storage s = swaps[id];
        if (msg.sender != s.maker) revert NotMaker();
        if (s.state != State.Open) revert BadState();
        if (block.timestamp >= s.t0) revert TooLate();
        s.state = State.Ready;
        emit Ready(id);
    }

    /// @notice Anyone may submit the taker's share; funds always go to the fixed payout.
    function claim(bytes32 id, uint256 a) external {
        Swap storage s = swaps[id];
        State st = s.state;
        if (st != State.Open && st != State.Ready) revert BadState();
        if (block.timestamp >= s.t1) revert TooLate();
        if (st == State.Open && block.timestamp < s.t0) revert TooEarly();
        if (!Pallas.isDiscreteLogOf(a, s.ax, s.ay)) revert BadSecret();
        s.state = State.Claimed;
        uint256 fee = s.relayerFee;
        if (fee != 0 && !IERC20(s.token).transfer(msg.sender, fee)) revert TransferFailed();
        if (!IERC20(s.token).transfer(s.payout, s.amount - fee)) revert TransferFailed();
        emit Claimed(id, a);
    }

    /// @notice Returns the tokens to the maker and publishes b.
    function refund(bytes32 id, uint256 b) external {
        Swap storage s = swaps[id];
        State st = s.state;
        if (st != State.Open && st != State.Ready) revert BadState();
        bool early = st == State.Open && block.timestamp < s.t0;
        if (!early && block.timestamp < s.t1) revert TooEarly();
        if (!Pallas.isDiscreteLogOf(b, s.bx, s.by)) revert BadSecret();
        s.state = State.Refunded;
        if (!IERC20(s.token).transfer(s.maker, s.amount)) revert TransferFailed();
        emit Refunded(id, b);
    }
}
