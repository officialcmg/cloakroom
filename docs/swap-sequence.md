# One Cloakroom swap, step by step

Taker sells ZEC for USDC; maker buys. The Zcash chain and the Base escrow sit in the middle, because the two
parties never talk directly: every message is an encrypted memo inside a shielded Zcash transaction, and
settlement goes through the escrow.

![Swap sequence](swap-sequence.png)

Source (Mermaid; GitHub renders it):

```mermaid
sequenceDiagram
    autonumber
    participant T as Taker
    participant Z as Zcash (shielded)
    participant E as Base escrow
    participant M as Maker

    Note over T,M: Setup: the maker publishes a signed directory entry (UA, RFQ key, EVM address)
    T->>T: check directory signature, fresh share a, A = [a]G, fresh payout address
    T->>Z: RFQ (encrypted memo): lots, A + proof, payout address
    Z-->>M: RFQ
    M->>M: fresh share b, B = [b]G
    M->>Z: Quote (encrypted, signed by maker EVM key): price, fee, premium, B + proof, t0/t1/t2/W
    Z-->>T: Quote
    T->>T: verify proofs and signature, R1q: W > F + B_R and t2 - open ≤ H
    T->>Z: Accept (encrypted) + premium
    Z-->>M: Accept + premium
    M->>E: open: lock USDC, store A, B, payout, deadlines
    E-->>T: escrow Open (taker waits until it is final, plus a cover delay)
    T->>Z: fund the joint address (spend key a + b) with the ZEC
    Z-->>M: joint note, k_Z confirmations (plus a cover delay)
    M->>E: setReady
    T->>E: commitClaim via relayer: c = H(id, a), signed by the payout key (reveals nothing)
    Note over T,E: wait until the lock is final
    T->>E: claim(a) via relayer: escrow checks H(id, a) = c and [a]G = A
    E-->>T: USDC (price - relayer fee) credited to the payout address, withdrawn
    E-->>M: Claimed event publishes a
    M->>Z: sweep the joint note with a + b to the maker's own wallet

    alt No funding seen by t0 (taker vanished, or funding still unmined)
        M->>E: refund(b): escrow checks [b]G = B, USDC back to maker
        E-->>T: Refunded event publishes b
        T->>Z: if the funding lands: sweep it back with a + b (taker keeps a until then)
    else Maker never sets Ready and never refunds
        Note over T,E: from t2 refunds are disabled
        T->>E: commitClaim on the punish path, then claim(a) as above
    end
```

What each side ends with:

| Outcome | Taker | Maker |
|---|---|---|
| Claimed | USDC at a fresh payout address | the ZEC, swept with a + b |
| Refunded | her ZEC back, minus the premium and fees | his USDC back, plus the premium |
