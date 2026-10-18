//! EVM leg of Cloakroom, driven through Foundry's `cast` (works against anvil or Base Sepolia).

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::process::Command;

#[derive(Clone)]
pub struct Evm {
    pub rpc: String,
    pub escrow: String,
    pub token: String,
    pub from_block: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    None,
    Open,
    Ready,
    Locked,
    Claimed,
    Refunded,
}

#[derive(Debug, Clone)]
pub struct SwapView {
    pub maker: String,
    pub payout: String,
    pub token: String,
    pub relayer: String,
    pub t0: u64,
    pub t1: u64,
    pub t2: u64,
    pub reveal_window: u64,
    pub locked_at: u64,
    pub state: State,
    pub amount: u64,
    pub relayer_fee: u64,
    pub ax: String,
    pub ay: String,
    pub bx: String,
    pub by: String,
    pub commitment: String,
}

fn cast(args: &[&str]) -> Result<String> {
    let out = Command::new("cast").args(args).output().context("running cast")?;
    if !out.status.success() {
        bail!("cast {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// cast prints numbers as "123 [1.23e2]"; keep the first token.
fn first(s: &str) -> String {
    s.split_whitespace().next().unwrap_or("").to_string()
}

const LOG_CHUNK: u64 = 200;

pub fn norm_hex(s: &str) -> String {
    let s = s.trim().trim_start_matches("0x").to_lowercase();
    format!("0x{:0>64}", s)
}

impl Evm {
    pub fn address_of(key: &str) -> Result<String> {
        Ok(cast(&["wallet", "address", "--private-key", key])?.to_lowercase())
    }

    pub fn send(&self, key: &str, to: &str, sig: &str, args: &[String]) -> Result<String> {
        let mut a: Vec<&str> = vec!["send", "--rpc-url", &self.rpc, "--private-key", key, "--json", to, sig];
        a.extend(args.iter().map(|s| s.as_str()));
        // Public RPCs can serve a stale pending nonce right after a previous send; retry.
        let mut tries = 0;
        let out = loop {
            match cast(&a) {
                Ok(o) => break o,
                Err(e) if tries < 8 && {
                    let m = e.to_string();
                    m.contains("nonce too low") || m.contains("replacement transaction underpriced") || m.contains("already known")
                } => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_secs(3));
                }
                Err(e) => return Err(e),
            }
        };
        let v: Value = serde_json::from_str(&out).context("cast send json")?;
        if v["status"].as_str() != Some("0x1") {
            bail!("tx reverted: {out}");
        }
        Ok(v["transactionHash"].as_str().unwrap_or_default().to_string())
    }

    pub fn call(&self, to: &str, sig: &str, args: &[String]) -> Result<Vec<String>> {
        let mut a: Vec<&str> = vec!["call", "--rpc-url", &self.rpc, to, sig];
        a.extend(args.iter().map(|s| s.as_str()));
        Ok(cast(&a)?.lines().map(first).collect())
    }

    pub fn now(&self) -> Result<u64> {
        Ok(first(&cast(&["block", "latest", "--field", "timestamp", "--rpc-url", &self.rpc])?).parse()?)
    }

    pub fn block_number(&self, tag: &str) -> Result<u64> {
        let s = first(&cast(&["block", tag, "--field", "number", "--rpc-url", &self.rpc])?);
        Ok(if let Some(h) = s.strip_prefix("0x") { u64::from_str_radix(h, 16)? } else { s.parse()? })
    }

    /// id = keccak256(abi.encode(maker, salt)), as in CloakroomEscrow.open.
    pub fn swap_id(maker: &str, salt: &str) -> Result<String> {
        let enc = cast(&["abi-encode", "f(address,bytes32)", maker, salt])?;
        Ok(norm_hex(&cast(&["keccak", &enc])?))
    }

    pub fn swap(&self, id: &str) -> Result<SwapView> {
        self.swap_at(id, None)
    }

    /// Escrow state for `id`, optionally as of a given block (e.g. the finalized head).
    pub fn swap_at(&self, id: &str, block: Option<u64>) -> Result<SwapView> {
        let sig = "swaps(bytes32)(address,address,address,address,uint64,uint64,uint64,uint64,uint64,uint8,uint256,uint256,uint256,uint256,uint256,uint256,bytes32)";
        let mut a: Vec<String> = vec!["call".into(), "--rpc-url".into(), self.rpc.clone()];
        if let Some(b) = block {
            a.push("--block".into());
            a.push(b.to_string());
        }
        a.extend([self.escrow.clone(), sig.into(), id.to_string()]);
        let refs: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        let v: Vec<String> = cast(&refs)?.lines().map(first).collect();
        if v.len() < 17 {
            bail!("unexpected swaps() output: {v:?}");
        }
        let state = match v[9].as_str() {
            "0" => State::None,
            "1" => State::Open,
            "2" => State::Ready,
            "3" => State::Locked,
            "4" => State::Claimed,
            "5" => State::Refunded,
            s => bail!("bad state {s}"),
        };
        Ok(SwapView {
            maker: v[0].to_lowercase(),
            payout: v[1].to_lowercase(),
            token: v[2].to_lowercase(),
            relayer: v[3].to_lowercase(),
            t0: v[4].parse()?,
            t1: v[5].parse()?,
            t2: v[6].parse()?,
            reveal_window: v[7].parse()?,
            locked_at: v[8].parse()?,
            state,
            amount: v[10].parse()?,
            relayer_fee: v[11].parse()?,
            ax: v[12].clone(),
            ay: v[13].clone(),
            bx: v[14].clone(),
            by: v[15].clone(),
            commitment: v[16].clone(),
        })
    }

    /// (number, timestamp) of a tagged block such as "finalized".
    pub fn block_info(&self, tag: &str) -> Result<(u64, u64)> {
        let n = self.block_number(tag)?;
        let t = first(&cast(&["block", &n.to_string(), "--field", "timestamp", "--rpc-url", &self.rpc])?).parse()?;
        Ok((n, t))
    }

    /// Sign the escrow's commit digest with the payout key; returns (v, r, s).
    pub fn sign_commit(&self, payout_key: &str, id: &str, commitment: &str, relayer: &str) -> Result<(String, String, String)> {
        let digest = self.call(&self.escrow, "commitDigest(bytes32,bytes32,address)(bytes32)", &[id.into(), commitment.into(), relayer.into()])?[0].clone();
        let sig = cast(&["wallet", "sign", "--no-hash", "--private-key", payout_key, &digest])?;
        let h = sig.trim_start_matches("0x");
        if h.len() != 130 {
            bail!("unexpected signature {sig}");
        }
        let v = u8::from_str_radix(&h[128..130], 16)?;
        let v = if v < 27 { v + 27 } else { v };
        Ok((v.to_string(), format!("0x{}", &h[..64]), format!("0x{}", &h[64..128])))
    }

    /// EIP-191 personal-message signature over a 32-byte digest, signed as its 64-char lowercase
    /// hex text (no 0x): `cast wallet sign` hex-decodes a 0x message but `cast wallet verify` does not,
    /// so a 0x-prefixed digest never verifies. 65-byte sig out.
    pub fn personal_sign(key: &str, digest: &[u8; 32]) -> Result<Vec<u8>> {
        let sig = cast(&["wallet", "sign", "--private-key", key, &hex::encode(digest)])?;
        let b = hex::decode(sig.trim().trim_start_matches("0x"))?;
        if b.len() != 65 {
            bail!("unexpected signature length {}", b.len());
        }
        Ok(b)
    }

    /// True iff `sig` is `addr`'s EIP-191 signature over `digest`.
    pub fn personal_verify(addr: &str, digest: &[u8; 32], sig: &[u8]) -> bool {
        cast(&["wallet", "verify", "--address", addr, &hex::encode(digest), &format!("0x{}", hex::encode(sig))]).is_ok()
    }

    pub fn commitment_of(&self, id: &str, a: &str) -> Result<String> {
        Ok(self.call(&self.escrow, "commitmentOf(bytes32,uint256)(bytes32)", &[id.into(), a.into()])?[0].clone())
    }

    /// Block number of the Locked event for `id`, if any.
    pub fn locked_block(&self, id: &str) -> Result<Option<u64>> {
        Ok(self.log_for("Locked(bytes32,bytes32,address)", id)?.map(|log| {
            let bn = log["blockNumber"].as_str().unwrap_or("0x0");
            u64::from_str_radix(bn.trim_start_matches("0x"), 16).unwrap_or(0)
        }))
    }

    /// The uint256 published by Claimed(id, a) or Refunded(id, b), with its block number.
    pub fn revealed(&self, event: &str, id: &str) -> Result<Option<(String, u64)>> {
        let sig = format!("{event}(bytes32,uint256)");
        let Some(log) = self.log_for(&sig, id)? else { return Ok(None) };
        let data = log["data"].as_str().ok_or_else(|| anyhow!("log data"))?;
        let bn = log["blockNumber"].as_str().unwrap_or("0x0");
        let bn = u64::from_str_radix(bn.trim_start_matches("0x"), 16).unwrap_or(0);
        Ok(Some((norm_hex(data), bn)))
    }

    /// Block number of the Opened event for `id`, if any.
    pub fn opened_block(&self, id: &str) -> Result<Option<u64>> {
        let sig = "Opened(bytes32,address,address,address,uint256,uint256,uint256,uint256,uint256,uint256,uint64,uint64,uint64,uint64)";
        Ok(self.log_for(sig, id)?.map(|log| {
            let bn = log["blockNumber"].as_str().unwrap_or("0x0");
            u64::from_str_radix(bn.trim_start_matches("0x"), 16).unwrap_or(0)
        }))
    }

    /// The first log of event `sig` emitted by the escrow whose first indexed topic is `id`.
    /// The topic is matched here explicitly rather than trusting the RPC-side filter.
    fn log_for(&self, sig: &str, id: &str) -> Result<Option<Value>> {
        let topic0 = norm_hex(&cast(&["keccak", sig])?);
        let want = norm_hex(id);
        let latest = self.block_number("latest")?;
        // Public RPCs cap eth_getLogs ranges (Base Sepolia: 200 blocks), so query in chunks.
        let mut lo = self.from_block;
        while lo <= latest {
            let hi = (lo + LOG_CHUNK - 1).min(latest);
            let (from, to) = (lo.to_string(), hi.to_string());
            let out = cast(&["logs", "--rpc-url", &self.rpc, "--from-block", &from, "--to-block", &to, "--address", &self.escrow, "--json", &topic0])?;
            let v: Value = serde_json::from_str(if out.is_empty() { "[]" } else { &out })?;
            if let Some(log) = v.as_array().into_iter().flatten().find(|log| {
                log["topics"].get(1).and_then(|t| t.as_str()).map(norm_hex).as_deref() == Some(want.as_str())
            }) {
                return Ok(Some(log.clone()));
            }
            lo = hi + 1;
        }
        Ok(None)
    }

    pub fn token_balance(&self, who: &str) -> Result<u64> {
        Ok(self.call(&self.token, "balanceOf(address)(uint256)", &[who.to_string()])?[0].parse()?)
    }
}
