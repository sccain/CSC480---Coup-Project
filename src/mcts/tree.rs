//! Monte Carlo search utilities.
//!
//! This module implements a lightweight **Information-Set MCTS** style root search:
//! - Each iteration samples a hidden state ("determinization") consistent with public info.
//! - We select a root action using UCB1.
//! - We run a playout from that action.
//! - Root action statistics are aggregated across determinizations.
//!
//! Note: this is root-focused (not a full shared tree keyed by information sets),
//! but it already removes the biggest determinization-leakage weakness.

use rand::prelude::*;

use crate::bot::Context;
use crate::Action;

use super::sim_state::{PlayoutPolicy, SimState};

/// Config for the MCTS root search.
#[derive(Clone, Copy, Debug)]
pub struct MctsConfig {
    /// Total number of MCTS iterations (in addition to the 1-per-action warmup).
    pub iterations: usize,
    /// Maximum playout depth (atomic transitions).
    pub max_depth: usize,
    /// Exploration constant in UCB1.
    pub exploration_c: f32,
    /// Risk penalty (mean - risk_lambda * stddev). Set to 0.0 for pure mean.
    pub risk_lambda: f32,
}

impl Default for MctsConfig {
    fn default() -> Self {
        Self {
            iterations: 1_200,
            max_depth: 120,
            exploration_c: 1.25,
            risk_lambda: 0.25,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct RootStats {
    n: u32,
    w_sum: f32,
    w_sq_sum: f32,
}

impl RootStats {
    fn add(&mut self, w: f32) {
        self.n = self.n.saturating_add(1);
        self.w_sum += w;
        self.w_sq_sum += w * w;
    }

    fn mean(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            self.w_sum / (self.n as f32)
        }
    }

    fn stddev(&self) -> f32 {
        if self.n <= 1 {
            return 0.0;
        }
        let n = self.n as f32;
        let mean = self.w_sum / n;
        let var = (self.w_sq_sum / n - mean * mean).max(0.0);
        var.sqrt()
    }
}

/// Information-set MCTS root searcher.
pub struct Mcts {
    cfg: MctsConfig,
}

impl Mcts {
    pub fn new(cfg: MctsConfig) -> Self {
        Self { cfg }
    }

    /// Choose the best action for `context.name`.
    pub fn search<P: PlayoutPolicy>(&self, context: &Context, policy: &P) -> Option<Action> {
        let mut rng = thread_rng();

        // Determine root legal actions (targets expanded) via a determinized state.
        let root = SimState::from_context_determinized(context, policy, &mut rng);
        let actions = root.legal_root_actions();
        if actions.is_empty() {
            return None;
        }

        // IMPORTANT: do NOT use HashMap<Action,...> (Action doesn't derive Hash in your project).
        // Keep stats aligned with actions by index.
        let mut stats: Vec<RootStats> = vec![RootStats::default(); actions.len()];

        // Warm up: evaluate each action once.
        for (i, a) in actions.iter().enumerate() {
            let mut s = SimState::from_context_determinized(context, policy, &mut rng);
            let w = s.playout_from_root_action(a, policy, &mut rng, self.cfg.max_depth);
            stats[i].add(w);
        }

        for _ in 0..self.cfg.iterations {
            // UCB1 over root actions.
            let total_n = stats.iter().map(|s| s.n as f32).sum::<f32>().max(1.0);

            let mut pick_i: usize = 0;
            let mut best_ucb = f32::MIN;

            for (i, s) in stats.iter().enumerate() {
                if s.n == 0 {
                    pick_i = i;
                    best_ucb = f32::INFINITY;
                    break;
                }
                let mean = s.mean();
                let explore =
                    self.cfg.exploration_c * ((total_n.ln() / (s.n as f32)).max(0.0)).sqrt();
                let ucb = mean + explore;
                if ucb > best_ucb {
                    best_ucb = ucb;
                    pick_i = i;
                }
            }

            let pick_action = actions[pick_i].clone();

            // Fresh determinization per iteration.
            let mut s = SimState::from_context_determinized(context, policy, &mut rng);
            let w = s.playout_from_root_action(&pick_action, policy, &mut rng, self.cfg.max_depth);
            stats[pick_i].add(w);
        }

        // Risk-aware final selection: mean - lambda * stddev
        let mut best_action: Option<Action> = None;
        let mut best_score = f32::MIN;

        for (i, a) in actions.iter().cloned().enumerate() {
            let s = &stats[i];
            let score = s.mean() - self.cfg.risk_lambda * s.stddev();
            if score > best_score {
                best_score = score;
                best_action = Some(a);
            }
        }

        best_action
    }
}
