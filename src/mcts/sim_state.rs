// this is a simplified version of the game state used ONLY for simulation
// we’re not modeling full coup yet, just enough to simulate decisions

use crate::{Action, Card};
use crate::bot::{Context, OtherBot};

#[derive(Clone)]
pub struct SimState {
    pub my_name: String,
    pub my_cards: Vec<Card>,
    pub my_coins: u8,
    pub opponents: Vec<OtherBot>,
}

impl SimState {
    pub fn from_context(context: &Context) -> Self {
        Self {
            my_name: context.name.clone(),
            my_cards: context.cards.clone(),
            my_coins: context.coins,
            opponents: context.playing_bots.clone(),
        }
    }
    // return possible actions from current state
    // rn we keep it simple (income + coup when possible)
    pub fn legal_actions(&self) -> Vec<Action> {
        let mut actions = vec![Action::Income];

        if self.my_coins >= 7 {
            if let Some(target) = self.opponents.first() {
                actions.push(Action::Coup(target.name.clone()));
            }
        }

        actions
    }
    // simulate what happens after taking an action
    // this is VERY simplified and does not model full coup logic yet

    pub fn apply_action(&self, action: &Action) -> Self {
        let mut next = self.clone();

        match action {
            Action::Income => next.my_coins += 1,
            Action::Coup(_) => next.my_coins -= 7,
            _ => {}
        }

        next
    }

    pub fn is_terminal(&self) -> bool {
        self.opponents.is_empty()
    }
    // reward function for rollouts
    // rn mostly placeholder since we don’t simulate full elimination yet
    pub fn reward(&self) -> f32 {
        if self.is_terminal() {
            1.0
        } else {
            0.0
        }
    }
}
