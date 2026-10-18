-------------------------------- MODULE Cloakroom --------------------------------
(*****************************************************************************)
(* Formal model of the Cloakroom v2 cross-chain atomic swap (t2 "punish"    *)
(* window variant, as in Gugger's XMR-BTC swap).                            *)
(*                                                                          *)
(* Taker T holds shielded ZEC; Maker M holds ERC-20 tokens on an EVM chain. *)
(* A ZEC note at a joint address is spendable only by whoever knows BOTH    *)
(* shares a (T's) and b (M's).  The EVM escrow holds M's tokens and has two *)
(* exits, each of which PUBLISHES one share:                                *)
(*   claim(a)  -> T is paid; a becomes public, so M can sweep ZEC with a+b. *)
(*   refund(b) -> M is refunded; b becomes public, so T can recover ZEC.    *)
(*                                                                          *)
(* This module captures: discrete time; the EVM escrow state machine with   *)
(* reorg-able unfinalized blocks and a finality lag F; EVM inclusion delay   *)
(* D with reverting-on-bad-precondition; secrets leaking at BROADCAST (even  *)
(* if the tx later reverts/reorgs); ZEC funding that is reorg-able until k_Z *)
(* confirmations; and an adversary that may control at most one party plus   *)
(* the scheduler (inclusion delays, reorgs, and crashes/offline of honest    *)
(* parties, modelled by the absence of a fairness obligation).              *)
(*                                                                          *)
(* The contract guards modelled here mirror CloakroomEscrow.sol (t2 design):*)
(*   setReady:    Open & now<t0                          (maker)            *)
(*   commitClaim: (Ready & now<t1) OR now>=t2 , st in {Open,Ready}          *)
(*                                              (payout-key signed => taker) *)
(*   claim(a):    Locked & now < lockedAt+W             (knows a)           *)
(*   refund(b):   (Open & now<t0)                                            *)
(*               OR ((Open|Ready) & t1<=now<t2)                             *)
(*               OR (Locked & now>=lockedAt+W)          (knows b)           *)
(*****************************************************************************)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    MaxTime,     \* time horizon (discrete steps 0..MaxTime)
    T0, T1, T2,  \* escrow deadlines, require 0 < T0 < T1 < T2
    W,           \* reveal window length (claim must land within lockedAt+W)
    D,           \* max EVM inclusion delay (broadcast -> inclusion in [1,D])
    F,           \* EVM finality lag (a block older than F steps is final)
    KZ,          \* ZEC confirmations before the joint-note funding is final
    MarginC,     \* taker's safety margin on the claim/commit/fund windows
    MarginR,     \* maker's safety margin on the deadline refund
    MaxReorg,    \* bound on total reorgs (keeps the state space finite; a
                 \* finalized chain only ever suffers finitely many reorgs)
    ADV,         \* "none" | "maker" | "taker" : which party may deviate
    MUT,         \* mutation selector (see below); "none" is the real v2 design
    AssumeMakerOnline, \* TRUE: honest maker is online in its action windows
    MakerOnlineFrom,   \* honest maker is offline (cannot act) before this time; 0 = always online.
                       \* Setting it to the latest point L_M allows checks the refund budget.
    AssumeTakerOnline  \* TRUE: honest taker is online in its action windows

(* Mutation flags -- each turns off exactly one protective rule for Task 4. *)
mutClaimFromOpen   == MUT = "claimFromOpen"        \* v1: commit allowed from Open (no Ready, no t2)
mutNoT2            == MUT = "noT2"                  \* remove the punish window entirely
mutSmallMargin     == MUT = "smallMargin"          \* taker margins shrink to 0
mutNoFinalityRefund== MUT = "noFinalityRefund"     \* maker refunds without waiting for finality
mutRevealBeforeFin == MUT = "revealBeforeFinal"    \* taker reveals a before Locked is finalized
mutRefundNearT1    == MUT = "refundNearT1NoFinal"  \* maker fires the t1 refund with no finality
mutReadyAfterRefund== MUT = "readyAfterRefund"     \* maker may set Ready after b is public
mutNoResend        == MUT = "noResend"            \* maker does not re-send a dropped refund late

MakerHonest == ADV # "maker"
TakerHonest == ADV # "taker"

(* Effective claim-side margin (the mutation that makes it too small). *)
EMarginC == IF mutSmallMargin THEN 0 ELSE MarginC

VARIABLES
    now,       \* current time
    chain,     \* Seq of [st |-> State, at |-> time] : the EVM escrow block history
    mp,        \* [KINDS -> -1..MaxTime] : broadcast time of a pending tx, -1 = none
    leaked,    \* subset of {"a","b"} : shares that have become public
    zec,       \* "Unfunded"|"Pending"|"Final"|"SweptT"|"SweptM"
    zfundAt,   \* time the joint note was funded (-1 if not)
    zecWasFinal, \* history: did a final ZEC funding of the joint note ever exist?
    reorgs     \* count of reorgs so far (bounded by MaxReorg)

vars == <<now, chain, mp, leaked, zec, zfundAt, zecWasFinal, reorgs>>

KINDS  == {"setReady", "commit", "claim", "refund"}
States == {"None","Open","Ready","Locked","Claimed","Refunded"}

Max(S) == CHOOSE x \in S : \A y \in S : y <= x

(* ----- derived views of the EVM escrow ----- *)
TipState == IF chain = <<>> THEN "None" ELSE chain[Len(chain)].st
TipAt    == IF chain = <<>> THEN -1      ELSE chain[Len(chain)].at

(* lockedAt field of the escrow: the timestamp of the (unique, on any path)
   Locked block.  Read from the Locked block itself -- NOT the tip -- so it
   stays correct after a later Refunded/Claimed block is appended. *)
LockedIdx == IF \E i \in 1..Len(chain) : chain[i].st = "Locked"
             THEN CHOOSE i \in 1..Len(chain) : chain[i].st = "Locked"
             ELSE 0
LockedAt == IF LockedIdx = 0 THEN -1 ELSE chain[LockedIdx].at

(* Index of the last finalized block (now - at >= F); 0 means nothing final. *)
FinIdx ==
    IF chain = <<>> THEN 0
    ELSE LET S == {i \in 1..Len(chain) : now - chain[i].at >= F}
         IN IF S = {} THEN 0 ELSE Max(S)

FinalState == IF FinIdx = 0 THEN "None" ELSE chain[FinIdx].st

(* A finalized Claimed / Refunded block is permanent (reorgs only drop the
   unfinalized suffix), so these are sound monotone "it happened" predicates. *)
EverFinClaimed  == \E i \in 1..Len(chain) : chain[i].st = "Claimed"  /\ now - chain[i].at >= F
EverFinRefunded == \E i \in 1..Len(chain) : chain[i].st = "Refunded" /\ now - chain[i].at >= F

ZecFunded == zec \in {"Pending","Final"}

(* ----- contract preconditions, evaluated at inclusion time `now` ----- *)
CanSetReady == TipState = "Open" /\ now < T0

CanCommit ==
    \/ (TipState = "Ready" /\ now < T1)
    \/ (~mutNoT2 /\ TipState \in {"Open","Ready"} /\ now >= T2)        \* punish path
    \/ (mutClaimFromOpen /\ TipState = "Open" /\ now < T1)             \* v1 bug

CanClaim == TipState = "Locked" /\ now < LockedAt + W

CanRefund ==
    \/ (TipState = "Open" /\ now < T0)
    \/ (TipState \in {"Open","Ready"} /\ now >= T1 /\ (mutNoT2 \/ now < T2))
    \/ (TipState = "Locked" /\ now >= LockedAt + W)

Precond(k) == CASE k = "setReady" -> CanSetReady
                [] k = "commit"   -> CanCommit
                [] k = "claim"    -> CanClaim
                [] k = "refund"   -> CanRefund

ResultState(k) == CASE k = "setReady" -> "Ready"
                    [] k = "commit"   -> "Locked"
                    [] k = "claim"    -> "Claimed"
                    [] k = "refund"   -> "Refunded"

FullyDone ==
    /\ (EverFinClaimed \/ EverFinRefunded)
    /\ zec \in {"Unfunded","SweptT","SweptM"}
    /\ \A k \in KINDS : mp[k] = -1

(* Final-accounting safety is evaluated at genuine protocol termination: the
   finalized chain has settled, the ZEC note is resolved, and no tx is in
   flight.  (Evaluating at the artificial time horizon instead would misread an
   honest operation that is merely still in flight as a loss; liveness, checked
   separately under fairness, guarantees this terminal is actually reached.) *)
Terminated == FullyDone

-----------------------------------------------------------------------------
(* The honest maker's deadline refund (Open/Ready in [t1,t2)) must satisfy two
   finality conditions:
     - a FINALIZED block past t1 already shows Open/Ready (now >= t1 + D + F),
       so no commit broadcast before t1 can still appear; and
     - the refund itself FINALIZES before t2 (broadcast by t2 - D - F - MarginR),
       so the adversary cannot reorg it out and let the taker punish-commit at t2.
   The honest maker re-broadcasts (MakerRefundOwed urgency) if a reorg drops it,
   which the gap t2 - t1 >= 2D + 2F + MarginR + 1 leaves room for. *)
HonestRefundDeadline ==
    IF (mutNoFinalityRefund \/ mutRefundNearT1)
    THEN (TipState \in {"Open","Ready"} /\ now >= T1 /\ now < T2)      \* buggy: tip-based, no finality
    ELSE (FinalState \in {"Open","Ready"} /\ now >= T1 + D + F /\ now <= T2 - D - F - MarginR)

(* Online-honesty urgency: an online honest party does not let time pass while
   it has a loss-avoiding protocol step to take.  Modelled by blocking Tick.
   Turning the knob off (AssumeX Online = FALSE) lets the scheduler delay that
   party past its window -- i.e. models it crashed/offline for that stretch. *)

(* Re-send of a refund that a reorg dropped: once b is public, the maker re-broadcasts whenever
   the escrow precondition can still hold.  The budget (t2 - D - F - MarginR) limits only the FIRST
   broadcast; re-sends are what Lemma 2 (budgeted transactions) relies on.  Without this disjunct
   TLC finds, at D = F = 2, a refund broadcast at its last moment, reorged out, never re-sent, and
   a free claim at t2 (round-2 review). *)
RefundResend ==
    /\ "b" \in leaked /\ ~mutNoResend
    /\ \/ (TipState = "Open" /\ now < T0)
       \/ (TipState \in {"Open","Ready"} /\ now >= T1 /\ now < T2)

(* The finality-gated deadline refund the honest maker owes an unfunded/idle
   swap (NOT the optional early refund).  Must land before t2. *)
MakerRefundOwed ==
    /\ mp["refund"] = -1
    /\ ~EverFinRefunded
    /\ \/ HonestRefundDeadline
       \/ RefundResend
       \/ (FinalState = "Locked"
            /\ (IF mutNoFinalityRefund THEN now >= LockedAt + W
                                       ELSE now - F >= LockedAt + W))

(* A windowed honest action is forced at its LAST safe moment -- the last step
   at which its honest guard still holds -- so an online party never lets the
   window lapse, yet earlier action is still allowed and a too-small margin
   still lands the action too late (the mutation still fails). *)
TakerCommitUrgent ==
    /\ zec = "Final" /\ mp["commit"] = -1
    /\ \/ (FinalState = "Ready" /\ TipState = "Ready" /\ now + EMarginC = T1 - 1)  \* last safe normal commit
       \/ (~mutNoT2 /\ FinalState \in {"Open","Ready"} /\ now >= T2)               \* punish path from t2

TakerRevealUrgent ==
    /\ "a" \notin leaked /\ mp["claim"] = -1
    /\ FinalState = "Locked" /\ now + EMarginC = LockedAt + W - 1                   \* last safe reveal

TakerUrgent ==
    /\ TakerHonest /\ AssumeTakerOnline
    /\ \/ ("a" \in leaked /\ TipState = "Locked" /\ mp["claim"] = -1 /\ now < LockedAt + W)  \* retry
       \/ ("b" \in leaked /\ ZecFunded)                 \* recover ZEC once b is public
       \/ TakerCommitUrgent
       \/ TakerRevealUrgent

MakerSetReadyUrgent ==
    /\ FinalState = "Open" /\ zec = "Final" /\ TipState = "Open" /\ mp["setReady"] = -1
    /\ (mutReadyAfterRefund \/ "b" \notin leaked)
    /\ now + D = T0 - 1                                  \* last safe moment to set Ready

MakerUp == MakerHonest => now >= MakerOnlineFrom

MakerUrgent ==
    /\ MakerHonest /\ AssumeMakerOnline /\ now >= MakerOnlineFrom
    /\ \/ ("a" \in leaked /\ ZecFunded)                 \* sweep ZEC once a is public
       \/ MakerRefundOwed
       \/ MakerSetReadyUrgent

UrgentPending == TakerUrgent \/ MakerUrgent


-----------------------------------------------------------------------------
Init ==
    /\ now = 0
    /\ chain = <<>>
    /\ mp = [k \in KINDS |-> -1]
    /\ leaked = {}
    /\ zec = "Unfunded"
    /\ zfundAt = -1
    /\ zecWasFinal = FALSE
    /\ reorgs = 0

-----------------------------------------------------------------------------
(* === Time === *)
(* A broadcast tx is included within [bt+1, bt+D]; time may not pass its
   inclusion deadline while it is still pending (the scheduler chooses WHEN to
   include within the window, but inclusion by bt+D is mandatory -- D is the
   max delay, not an option to drop the tx). *)
MustIncludeNow == \E k \in KINDS : mp[k] # -1 /\ now >= mp[k] + D

(* The ZEC note gains k_Z confirmations in bounded time: once KZ steps have
   passed since funding it becomes final (the adversary may reorg it only
   before then).  Finality is not an option the scheduler can defer forever. *)
MustPromoteZec == zec = "Pending" /\ now >= zfundAt + KZ

Tick ==
    /\ now < MaxTime
    /\ ~FullyDone
    /\ ~MustIncludeNow
    /\ ~MustPromoteZec
    /\ ~UrgentPending        \* an online honest party must act before time passes
    /\ now' = now + 1
    /\ UNCHANGED <<chain, mp, leaked, zec, zfundAt, zecWasFinal, reorgs>>

(* === Maker opens the escrow (immediate; its finality is still modelled via
       the F lag before honest T funds). === *)
Open ==
    /\ chain = <<>>
    /\ now < T0
    /\ chain' = <<[st |-> "Open", at |-> now]>>
    /\ UNCHANGED <<now, mp, leaked, zec, zfundAt, zecWasFinal, reorgs>>

(* === Broadcasts: enter the mempool, leak the carried secret immediately. ===*)

(* b \notin leaked: once the maker has broadcast a refund (b public), it never sets Ready.
   Without this, an early refund near t0 and a late funding by a malicious taker overlap when
   D > 1: Ready lands, the taker locks and claims, and also sweeps the ZEC with b (TLC, D=F=2). *)
HonestSetReadyGuard == FinalState = "Open" /\ zec = "Final" /\ now + D < T0 /\ (mutReadyAfterRefund \/ "b" \notin leaked)

BcastSetReady ==
    /\ chain # <<>>
    /\ mp["setReady"] = -1
    /\ (MakerHonest => HonestSetReadyGuard)   \* malicious maker: any time
    /\ MakerUp
    /\ mp' = [mp EXCEPT !["setReady"] = now]
    /\ UNCHANGED <<now, chain, leaked, zec, zfundAt, zecWasFinal, reorgs>>

(* An honest taker only commits on a swap whose joint note she has funded to
   finality (k_Z confirmations, no longer reorg-able); otherwise she has nothing
   dependable at stake and claiming would be theft / a free claim. *)
HonestCommitGuard ==
    /\ zec = "Final"
    /\ \/ (FinalState = "Ready" /\ now + EMarginC < T1)                \* normal
       \/ (~mutNoT2 /\ FinalState \in {"Open","Ready"} /\ now >= T2)   \* punish

BcastCommit ==
    /\ chain # <<>>
    /\ mp["commit"] = -1
    /\ (TakerHonest => HonestCommitGuard)     \* only the payout key (taker) can commit
    /\ mp' = [mp EXCEPT !["commit"] = now]
    /\ UNCHANGED <<now, chain, leaked, zec, zfundAt, zecWasFinal, reorgs>>

(* The taker reveals a only once (initial reveal) when the Locked block is
   final and there is margin left in the window; thereafter, since a is already
   public, she *retries* the claim (as the relayer does) whenever the escrow is
   Locked and the raw window is still open, to push it through any reorg of her
   Claimed block.  Margin must cover inclusion + the worst-case reorg delay. *)
HonestClaimGuard ==
    /\ now < LockedAt + W
    /\ \/ ( (IF mutRevealBeforeFin THEN TipState = "Locked" ELSE FinalState = "Locked")
            /\ now + EMarginC < LockedAt + W )           \* initial reveal, with margin
       \/ ( "a" \in leaked /\ TipState = "Locked" )       \* retry after a is public

BcastClaim ==
    /\ chain # <<>>
    /\ mp["claim"] = -1
    /\ (TakerHonest => HonestClaimGuard)
    /\ mp' = [mp EXCEPT !["claim"] = now]
    /\ leaked' = leaked \cup {"a"}            \* a leaks at broadcast
    /\ UNCHANGED <<now, chain, zec, zfundAt, zecWasFinal, reorgs>>

(* The early (pre-t0) refund may publish b, so it is fired only once the
   taker's own safe-funding window has already closed (now + EMarginC >= T0).
   This keeps the "taker funds" and "maker reveals b early" windows disjoint,
   so b can never become public while the taker could still fund-and-both-exit. *)
HonestRefundGuard ==
    \/ (FinalState = "Open" /\ now < T0 /\ zec = "Unfunded"
          /\ now + EMarginC >= T0)                                    \* early, note absent, near t0
    \/ HonestRefundDeadline                                            \* Open/Ready in [t1,t2)
    \/ RefundResend                                                    \* re-send a dropped refund
    \/ (FinalState = "Locked"
         /\ (IF mutNoFinalityRefund THEN now >= LockedAt + W
                                    ELSE now - F >= LockedAt + W))      \* Locked past window

BcastRefund ==
    /\ chain # <<>>
    /\ mp["refund"] = -1
    /\ (MakerHonest => HonestRefundGuard)
    /\ MakerUp
    /\ mp' = [mp EXCEPT !["refund"] = now]
    /\ leaked' = leaked \cup {"b"}            \* b leaks at broadcast
    /\ UNCHANGED <<now, chain, zec, zfundAt, zecWasFinal, reorgs>>

(* === Inclusion: within [bt+1, bt+D]; appends if the precondition holds at
       inclusion time, otherwise reverts (tx dropped, secret already leaked). ===*)
Include(k) ==
    /\ mp[k] # -1
    /\ now > mp[k]
    /\ now <= mp[k] + D
    /\ mp' = [mp EXCEPT ![k] = -1]
    /\ IF Precond(k)
         THEN chain' = Append(chain, [st |-> ResultState(k), at |-> now])
         ELSE chain' = chain
    /\ UNCHANGED <<now, leaked, zec, zfundAt, zecWasFinal, reorgs>>

(* === EVM reorg: drop an unfinalized suffix of the chain. === *)
ReorgEVM ==
    /\ reorgs < MaxReorg
    /\ \E j \in 0..(Len(chain) - 1) :
         /\ \A i \in (j+1)..Len(chain) : now - chain[i].at < F
         /\ chain' = SubSeq(chain, 1, j)
    /\ reorgs' = reorgs + 1
    /\ UNCHANGED <<now, mp, leaked, zec, zfundAt, zecWasFinal>>

(* === ZEC joint note === *)
HonestFundGuard == FinalState = "Open" /\ now + EMarginC < T0

FundZec ==
    /\ zec = "Unfunded"
    /\ chain # <<>>
    /\ (TakerHonest => HonestFundGuard)       \* adversarial taker may fund anytime / never
    /\ zec' = "Pending"
    /\ zfundAt' = now
    /\ UNCHANGED <<now, chain, mp, leaked, zecWasFinal, reorgs>>

PromoteZec ==
    /\ zec = "Pending"
    /\ now - zfundAt >= KZ
    /\ zec' = "Final"
    /\ zecWasFinal' = TRUE          \* history: a final ZEC funding existed
    /\ UNCHANGED <<now, chain, mp, leaked, zfundAt, reorgs>>

ReorgZec ==
    /\ reorgs < MaxReorg
    /\ zec = "Pending"
    /\ now - zfundAt < KZ
    /\ zec' = "Unfunded"
    /\ zfundAt' = -1
    /\ reorgs' = reorgs + 1
    /\ UNCHANGED <<now, chain, mp, leaked, zecWasFinal>>

(* Whoever knows both shares can sweep the funded note; first sweep wins. *)
SweepT ==
    /\ ZecFunded
    /\ "b" \in leaked
    /\ zec' = "SweptT"
    /\ UNCHANGED <<now, chain, mp, leaked, zfundAt, zecWasFinal, reorgs>>

SweepM ==
    /\ ZecFunded
    /\ "a" \in leaked
    /\ MakerUp
    /\ zec' = "SweptM"
    /\ UNCHANGED <<now, chain, mp, leaked, zfundAt, zecWasFinal, reorgs>>

Next ==
    \/ Tick
    \/ Open
    \/ BcastSetReady \/ BcastCommit \/ BcastClaim \/ BcastRefund
    \/ \E k \in KINDS : Include(k)
    \/ ReorgEVM
    \/ FundZec \/ PromoteZec \/ ReorgZec
    \/ SweepT \/ SweepM

(* Fairness: honest parties and the honest-inclusion scheduler make progress;
   the adversary's reorgs are bounded (MaxReorg), so time can always advance. *)
Fairness ==
    /\ WF_vars(Tick)
    /\ WF_vars(Open)
    /\ (MakerHonest => WF_vars(BcastSetReady) /\ WF_vars(BcastRefund) /\ WF_vars(SweepM))
    /\ (TakerHonest => WF_vars(BcastCommit) /\ WF_vars(BcastClaim)
                       /\ WF_vars(FundZec)  /\ WF_vars(SweepT))
    /\ \A k \in KINDS : WF_vars(Include(k))
    /\ WF_vars(PromoteZec)

SpecSafety == Init /\ [][Next]_vars
SpecLive   == Init /\ [][Next]_vars /\ Fairness

-----------------------------------------------------------------------------
(* === Invariants (Task 2) === *)
TypeOK ==
    /\ now \in 0..MaxTime
    /\ mp \in [KINDS -> (-1)..MaxTime]
    /\ leaked \subseteq {"a","b"}
    /\ zec \in {"Unfunded","Pending","Final","SweptT","SweptM"}
    /\ zfundAt \in (-1)..MaxTime
    /\ reorgs \in 0..MaxReorg
    /\ \A i \in 1..Len(chain) : chain[i].st \in States

(* claim and refund never both finalize *)
NoDoubleExit == ~(EverFinClaimed /\ EverFinRefunded)

(* Tokens are credited to T only if a final ZEC funding of the joint note ever
   existed.  (In the t2 design the punish path lets a claim follow directly
   from Open/Ready, so this is deliberately phrased as "a final funding existed"
   rather than the stronger "...when Ready was set", which only holds for the
   no-t2 design and is checked separately as NoFreeClaimAtReady.) *)
NoFreeClaim == EverFinClaimed => zecWasFinal

(* The taker can still obtain the tokens: the escrow is Locked inside the reveal
   window (finalized, so it cannot be reorged away) or her claim is already in
   flight.  Such a state is not yet a loss -- it resolves to Claimed. *)
ClaimStillRedeemable ==
    \/ mp["claim"] # -1
    \/ (TipState = "Locked" /\ now < LockedAt + W)

(* An honest taker never ends with the maker holding her ZEC and no tokens,
   with no way left to obtain the tokens. *)
TakerLostBoth == zec = "SweptM" /\ ~EverFinClaimed /\ ~ClaimStillRedeemable
HonestTakerSafe == (TakerHonest /\ Terminated) => ~TakerLostBoth

(* An honest maker never ends with the taker holding the tokens while the
   maker can get neither the tokens (refund) nor the ZEC (sweep with a). *)
MakerLostBoth ==
    /\ EverFinClaimed
    /\ zec # "SweptM"
    /\ ~("a" \in leaked /\ ZecFunded)
HonestMakerSafe == (MakerHonest /\ Terminated) => ~MakerLostBoth

(* A plain finite-horizon bound, used as the state constraint elsewhere. *)
Horizon == now <= MaxTime

-----------------------------------------------------------------------------
(* === Liveness (Task 3) === *)
(* Once the escrow Open is finalized (the swap has really started and cannot be
   reorged back to nothing), it always settles (Claimed or Refunded). *)
EverFinOpen == \E i \in 1..Len(chain) : chain[i].st = "Open" /\ now - chain[i].at >= F
SwapSettles == EverFinOpen ~> (EverFinClaimed \/ EverFinRefunded)

(* No permanent hostage: an honest taker's ZEC never stays locked forever
   without her getting the tokens.  She ends up with tokens (Claimed), or her
   ZEC back / never parted with (Unfunded/SweptT), or the maker took it in a
   completed swap (SweptM, which implies she was paid on the happy path). *)
NoHostage ==
    ZecFunded ~> (EverFinClaimed \/ zec \in {"Unfunded","SweptT","SweptM"})
=============================================================================
