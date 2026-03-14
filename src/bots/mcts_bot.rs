// mcts_bot.rs
use crate::bot::{BotInterface, Context};
use crate::Action;

use crate::bots::duel_brain::DuelBrain;
use crate::mcts::sim_state::PlayoutPolicy;
use crate::mcts::tree::{Mcts, MctsConfig};

use std::cell::RefCell;

pub struct MctsBot {
    brain: RefCell<DuelBrain>,
}

impl Default for MctsBot {
    fn default() -> Self {
        Self {
            brain: RefCell::new(DuelBrain::new()),
        }
    }
}

struct BrainPolicy<'a> {
    brain: &'a RefCell<DuelBrain>,
}

impl<'a> PlayoutPolicy for BrainPolicy<'a> {
    fn decide_turn(&self, _player: &str, ctx: &Context) -> Action {
        let mut b = self.brain.borrow_mut();
        DuelBrain::decide_turn(&mut *b, ctx)
    }

    fn decide_counter(&self, player: &str, action: &Action, by: &str, ctx: &Context) -> bool {
        // `ctx.name` is the player whose perspective this context represents.
        // The adapter is called with `player` for clarity; if it mismatches, trust ctx.
        let mut b = self.brain.borrow_mut();
        DuelBrain::decide_counter(&mut *b, action, by, ctx)
    }

    fn decide_challenge_action(&self, _player: &str, action: &Action, by: &str, ctx: &Context) -> bool {
        let mut b = self.brain.borrow_mut();
        DuelBrain::decide_challenge_action(&mut *b, action, by, ctx)
    }

    fn decide_challenge_counter(&self, _player: &str, action: &Action, by: &str, ctx: &Context) -> bool {
        let mut b = self.brain.borrow_mut();
        DuelBrain::decide_challenge_counter(&mut *b, action, by, ctx)
    }

    fn choose_influence_to_lose(&self, _player: &str, ctx: &Context) -> Option<crate::Card> {
        <DuelBrain as PlayoutPolicy>::choose_influence_to_lose(&*self.brain.borrow(), &ctx.name, ctx)
    }
}

impl BotInterface for MctsBot {
    fn get_name(&self) -> String {
        String::from("MCTSBot")
    }

    fn on_turn(&self, context: &Context) -> Action {
        let cfg = MctsConfig {
            iterations: 1_500,
            max_depth: 140,
            exploration_c: 1.20,
            risk_lambda: 0.22,
        };
        let mcts = Mcts::new(cfg);
        let policy = BrainPolicy { brain: &self.brain };

        // Important: we pass the REAL context (with real public history) to the search;
        let action = mcts.search(context, &policy).unwrap_or(Action::Income);
        action
    }

    fn on_counter(&self, action: &Action, by: String, context: &Context) -> bool {
        DuelBrain::decide_counter(&mut *self.brain.borrow_mut(), action, &by, context)
    }

    fn on_challenge_action_round(&self, action: &Action, by: String, context: &Context) -> bool {
        DuelBrain::decide_challenge_action(&mut *self.brain.borrow_mut(), action, &by, context)
    }

    fn on_challenge_counter_round(&self, action: &Action, by: String, context: &Context) -> bool {
        DuelBrain::decide_challenge_counter(&mut *self.brain.borrow_mut(), action, &by, context)
    }
}
