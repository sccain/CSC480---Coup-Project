use crate::bot::{BotInterface, Context};
use crate::Action;

use crate::mcts::sim_state::SimState;
use crate::mcts::tree::Mcts;

use std::cell::RefCell;
use crate::bots::duel_brain::DuelBrain;

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

impl BotInterface for MctsBot {
    fn get_name(&self) -> String {
        String::from("MCTSBot")
    }

    fn on_turn(&self, context: &Context) -> Action {
        println!("MCTSBot deciding...");

        let mut rng = rand::thread_rng();
        let sim = SimState::from_context_with_sampled_opponents(context, &mut rng);

        let mcts = Mcts::new(sim);

        // iterations here are per candidate action; tune this
        let action = mcts.search(200).unwrap_or(Action::Income);

        println!("MCTSBot chose: {:?}", action);
        action
    }

    fn on_counter(&self, action: &Action, by: String, context: &Context) -> bool {
        self.brain.borrow_mut().decide_counter(action, &by, context)
    }
}
