use rand::prelude::*;

use super::sim_state::SimState;
use crate::Action;

use crate::bots::duel_brain::DuelBrain;

pub struct Mcts {
    root: SimState,
}

impl Mcts {
    pub fn new(state: SimState) -> Self {
        Self { root: state }
    }

    pub fn search(&self, iterations: usize) -> Option<Action> {
        let mut rng = thread_rng();
        let actions = self.root.legal_actions_for(self.root.to_move);
        if actions.is_empty() {
            return None;
        }

        let mut best_action = None;
        let mut best_score = f32::MIN;

        for action in actions {
            let mut total = 0.0;
            for _ in 0..iterations {
                total += self.rollout_with_policy(&action, &mut rng);
            }
            if total > best_score {
                best_score = total;
                best_action = Some(action);
            }
        }

        best_action
    }

    fn rollout_with_policy(&self, first_action: &Action, rng: &mut ThreadRng) -> f32 {
        // Per-rollout brains (no shared memory contamination)
        let mut brains: Vec<DuelBrain> = (0..self.root.players.len()).map(|_| DuelBrain::new()).collect();

        let mut s = self.root.clone();

        // apply the candidate first action for root
        s.step(first_action, rng);

        // simulate forward
        let max_depth = 30;
        for _ in 0..max_depth {
            if s.is_terminal() {
                return s.reward_for_root();
            }

            // if current player dead, step() will advance; but we still keep loop simple
            let pi = s.to_move;
            if s.players[pi].cards.is_empty() {
                s.to_move = (s.to_move + 1) % s.players.len();
                continue;
            }

            let ctx = s.as_context_for_player(pi);

            // DuelBrain proposes an action; clamp to legal to avoid unsupported outputs
            let proposed = brains[pi].decide_turn(&ctx);
            let legal = s.legal_actions_for(pi);

            let chosen = if legal.iter().any(|a| a == &proposed) {
                proposed
            } else {
                // fallback: pick a random legal action
                legal[rng.gen_range(0..legal.len())].clone()
            };

            s.step(&chosen, rng);
        }

        // depth cutoff
        s.reward_for_root()
    }
}
