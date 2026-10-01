//! Cost estimates. Always labelled as estimates: subscription usage isn't
//! exposed by the tools, so users set their own effective prices.

use crate::types::{AgentPrice, ModelRef, Tier};

/// ~4 characters per token when a CLI doesn't report usage.
pub fn estimate_tokens(chars: usize) -> u64 {
    (chars as u64).div_ceil(4)
}

pub fn price_tokens(in_per_m: f64, out_per_m: f64, tin: u64, tout: u64) -> f64 {
    (tin as f64 * in_per_m + tout as f64 * out_per_m) / 1_000_000.0
}

/// Estimated cost of one step.
pub fn step_cost(tier: Tier, agent_price: Option<&AgentPrice>, model: Option<&ModelRef>, tin: u64, tout: u64) -> f64 {
    match tier {
        Tier::Local => 0.0,
        Tier::CheapCloud => {
            let m = model.cloned();
            let (i, o) = m.map(|m| (m.price_in_per_m.unwrap_or(0.0), m.price_out_per_m.unwrap_or(0.0))).unwrap_or((0.0, 0.0));
            price_tokens(i, o, tin, tout)
        }
        Tier::Premium => paid_cost(agent_price, tin, tout),
    }
}

/// What the same tokens would cost on the paid agent ("saved vs all-paid").
pub fn paid_cost(agent_price: Option<&AgentPrice>, tin: u64, tout: u64) -> f64 {
    match agent_price {
        Some(p) => {
            if let Some(r) = p.per_request {
                r
            } else {
                price_tokens(p.in_per_m.unwrap_or(3.0), p.out_per_m.unwrap_or(15.0), tin, tout)
            }
        }
        None => price_tokens(3.0, 15.0, tin, tout),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_is_free_and_paid_uses_prices() {
        assert_eq!(step_cost(Tier::Local, None, None, 1_000_000, 1_000_000), 0.0);
        let p = AgentPrice { in_per_m: Some(3.0), out_per_m: Some(15.0), per_request: None };
        assert!((step_cost(Tier::Premium, Some(&p), None, 1_000_000, 100_000) - 4.5).abs() < 1e-9);
        let sub = AgentPrice { per_request: Some(0.04), ..Default::default() };
        assert_eq!(paid_cost(Some(&sub), 5, 5), 0.04);
        assert_eq!(estimate_tokens(9), 3);
    }
}
