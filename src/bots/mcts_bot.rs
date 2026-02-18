// basically we’re trying to make the bot think ahead using simulations
// rn this is a simple rollout-based version (not full UCT yet)
// but it runs and integrates with the engine

use crate::bot::{BotInterface, Context};
use crate::Action;
use crate::mcts::sim_state::SimState;
use crate::mcts::tree::Mcts;

pub struct MctsBot;

impl BotInterface for MctsBot {
    fn get_name(&self) -> String {
        String::from("MCTSBot")
    }
    fn on_turn(&self, context: &Context) -> Action {
        println!("MCTSBot deciding...");
    
        // convert engine Context into our simplified simulation state
        // this is what MCTS actually reasons over
        let sim = SimState::from_context(context);

        // run a bunch of simulations to evaluate possible actions
        // more iterations = more thinking (but slower)
        let mut mcts = Mcts::new(sim);
        mcts.search(200);

        // pick the best action based on simulation results

    
        let action = mcts.best_action().unwrap_or(Action::Income);
    
        println!("MCTSBot chose: {:?}", action);
    
        action
    }
    
}
