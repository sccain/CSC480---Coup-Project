use rand::prelude::*;

use crate::{Action, Card};
use crate::bot::{Context, OtherBot};

#[derive(Clone)]
pub struct SimPlayer {
    pub name: String,
    pub coins: u8,
    pub cards: Vec<Card>, // hidden hand inside sim
}

#[derive(Clone)]
pub struct SimState {
    pub root_name: String,
    pub players: Vec<SimPlayer>, // index 0..n
    pub to_move: usize,
    pub discard_pile: Vec<Card>,
}

impl SimState {
    pub fn from_context_with_sampled_opponents(context: &Context, rng: &mut ThreadRng) -> Self {
        // Build remaining deck counts
        let mut deck: Vec<Card> = Vec::with_capacity(15);
        for _ in 0..3 {
            deck.push(Card::Duke);
            deck.push(Card::Assassin);
            deck.push(Card::Captain);
            deck.push(Card::Ambassador);
            deck.push(Card::Contessa);
        }

        // Remove visible: our hand + discard pile (engine-known)
        let mut remove_one = |c: Card| {
            if let Some(i) = deck.iter().position(|x| *x == c) {
                deck.swap_remove(i);
            }
        };
        for c in &context.cards {
            remove_one(*c);
        }
        for c in &context.discard_pile {
            remove_one(*c);
        }

        // Build players: root + opponents with sampled hidden cards
        let mut players = Vec::new();
        players.push(SimPlayer {
            name: context.name.clone(),
            coins: context.coins,
            cards: context.cards.clone(),
        });

        for ob in &context.playing_bots {
            if ob.name == context.name {
                continue;
            }
            let need = ob.cards as usize;
            let mut opp_cards = Vec::with_capacity(need);
            for _ in 0..need {
                if deck.is_empty() {
                    break;
                }
                let i = rng.gen_range(0..deck.len());
                opp_cards.push(deck.swap_remove(i));
            }
            players.push(SimPlayer {
                name: ob.name.clone(),
                coins: ob.coins,
                cards: opp_cards,
            });
        }

        Self {
            root_name: context.name.clone(),
            players,
            to_move: 0,
            discard_pile: context.discard_pile.clone(),
        }
    }

    pub fn is_terminal(&self) -> bool {
        // terminal if only one player has influence
        let alive = self.players.iter().filter(|p| !p.cards.is_empty()).count();
        alive <= 1
    }

    pub fn winner_name(&self) -> Option<String> {
        if !self.is_terminal() {
            return None;
        }
        self.players
            .iter()
            .find(|p| !p.cards.is_empty())
            .map(|p| p.name.clone())
    }

    pub fn reward_for_root(&self) -> f32 {
        if let Some(w) = self.winner_name() {
            return if w == self.root_name { 1.0 } else { 0.0 };
        }

        // Non-terminal heuristic if depth cutoff reached:
        // prefer more influence, then more coins.
        let root = self.players.iter().find(|p| p.name == self.root_name).unwrap();
        let opp_sum: i32 = self.players
            .iter()
            .filter(|p| p.name != self.root_name)
            .map(|p| p.cards.len() as i32)
            .sum();

        let inf = root.cards.len() as i32 - opp_sum;
        let coin = root.coins as i32;

        (0.5 + 0.08 * inf as f32 + 0.02 * coin as f32).clamp(0.0, 1.0)
    }

    pub fn current_player(&self) -> &SimPlayer {
        &self.players[self.to_move]
    }

    pub fn current_player_mut(&mut self) -> &mut SimPlayer {
        &mut self.players[self.to_move]
    }

    pub fn opponent_indices(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.players.len()).filter(move |i| *i != self.to_move && !self.players[*i].cards.is_empty())
    }

    fn pick_default_target(&self) -> Option<usize> {
        self.opponent_indices().next()
    }

    pub fn step(&mut self, action: &Action, rng: &mut ThreadRng) {
        if self.is_terminal() {
            return;
        }

        // dead player does nothing; advance turn
        if self.current_player().cards.is_empty() {
            self.to_move = (self.to_move + 1) % self.players.len();
            return;
        }

        match action {
            Action::Income => {
                self.current_player_mut().coins = self.current_player().coins.saturating_add(1);
            }
            Action::ForeignAid => {
                self.current_player_mut().coins = self.current_player().coins.saturating_add(2);
            }
            Action::Tax => {
                self.current_player_mut().coins = self.current_player().coins.saturating_add(3);
            }
            Action::Stealing(target_name) => {
                if let Some(ti) = self.players.iter().position(|p| &p.name == target_name) {
                    let steal_amt = self.players[ti].coins.min(2);
                    self.players[ti].coins -= steal_amt;
                    self.current_player_mut().coins += steal_amt;
                }
            }
            Action::Assassination(target_name) => {
                if self.current_player().coins >= 3 {
                    self.current_player_mut().coins -= 3;
                    if let Some(ti) = self.players.iter().position(|p| &p.name == target_name) {
                        self.lose_random_influence(ti, rng);
                    }
                }
            }
            Action::Coup(target_name) => {
                if self.current_player().coins >= 7 {
                    self.current_player_mut().coins -= 7;
                    if let Some(ti) = self.players.iter().position(|p| &p.name == target_name) {
                        self.lose_random_influence(ti, rng);
                    }
                }
            }
            // Ignore unsupported actions for rollouts
            _ => {}
        }

        // Next alive player
        self.to_move = (self.to_move + 1) % self.players.len();
    }

    fn lose_random_influence(&mut self, player_idx: usize, rng: &mut ThreadRng) {
        if self.players[player_idx].cards.is_empty() {
            return;
        }
        let i = rng.gen_range(0..self.players[player_idx].cards.len());
        let lost = self.players[player_idx].cards.swap_remove(i);
        self.discard_pile.push(lost);
    }

    /// Build an engine-like Context for policy evaluation from the sim.
    pub fn as_context_for_player(&self, player_idx: usize) -> Context {
        let me = &self.players[player_idx];

        let mut playing_bots = Vec::new();
        for p in &self.players {
            if p.name == me.name {
                continue;
            }
            playing_bots.push(OtherBot {
                name: p.name.clone(),
                coins: p.coins,
                cards: p.cards.len() as u8,
            });
        }

        Context {
            name: me.name.clone(),
            coins: me.coins,
            cards: me.cards.clone(),
            playing_bots,
            discard_pile: self.discard_pile.clone(),
            history: vec![], // rollouts do not model claim history
            score: vec![],
        }
    }

    /// A simple legal action set consistent with the sim’s supported step().
    pub fn legal_actions_for(&self, player_idx: usize) -> Vec<Action> {
        let me = &self.players[player_idx];
        if me.cards.is_empty() {
            return vec![];
        }

        let target_idx = self.pick_default_target();
        let target_name = target_idx.map(|i| self.players[i].name.clone());

        let mut a = vec![Action::Income, Action::ForeignAid, Action::Tax];

        if let Some(t) = &target_name {
            a.push(Action::Stealing(t.clone()));
        }

        if me.coins >= 3 {
            if let Some(t) = &target_name {
                a.push(Action::Assassination(t.clone()));
            }
        }
        if me.coins >= 7 {
            if let Some(t) = &target_name {
                a.push(Action::Coup(t.clone()));
            }
        }

        a
    }
}
