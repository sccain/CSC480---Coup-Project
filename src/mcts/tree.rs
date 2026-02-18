// this is our basic MCTS engine
// currently using rollout evaluation
// not full UCT tree expansion yet (skeleton is here though)

use rand::prelude::*;
use super::sim_state::SimState;
use crate::Action;

#[derive(Clone)]
struct Node {
    state: SimState,
    visits: u32,
    value: f32,
    children: Vec<Node>,
    action: Option<Action>,
}

impl Node {
    fn new(state: SimState) -> Self {
        Self {
            state,
            visits: 0,
            value: 0.0,
            children: vec![],
            action: None,
        }
    }
}

pub struct Mcts {
    root: Node,
}

impl Mcts {
    pub fn new(state: SimState) -> Self {
        Self {
            root: Node::new(state),
        }
    }
    // run multiple simulations to estimate action quality
    // each iteration performs a random rollout
    pub fn search(&mut self, iterations: usize) {
        let mut rng = thread_rng();

        for _ in 0..iterations {
            let reward = self.rollout(self.root.state.clone(), &mut rng);
            self.root.visits += 1;
            self.root.value += reward;
        }
    }
    // simulate random future actions until depth limit
    // this prevents infinite loops since our sim is incomplete
    fn rollout(&self, mut state: SimState, rng: &mut ThreadRng) -> f32 {
        for _ in 0..20 {   // limit depth to 20 steps
            if state.is_terminal() {
                return state.reward();
            }
    
            let actions = state.legal_actions();
            if actions.is_empty() {
                return 0.0;
            }
    
            let action = actions[rng.gen_range(0..actions.len())].clone();
            state = state.apply_action(&action);
        }
    
        0.0 // return neutral if depth limit reached
    }
    // choose the action that performed best in simulations
    // rn very simple evaluation logic
    pub fn best_action(&self) -> Option<Action> {
        let actions = self.root.state.legal_actions();
    
        let mut best_score = f32::MIN;
        let mut best_action = None;
    
        let mut rng = rand::thread_rng();
    
        for action in actions {
            let mut total = 0.0;
    
            for _ in 0..50 {
                let next_state = self.root.state.apply_action(&action);
                total += self.rollout(next_state.clone(), &mut rng);
            }
    
            if total > best_score {
                best_score = total;
                best_action = Some(action);
            }
        }
    
        best_action
    }
}
