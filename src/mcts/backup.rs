//! A Coup simulation model used by the MCTS bot.
//!
//! Design goals:
//! - Keep state transitions deterministic given a sampled hidden state.
//! - Model the phases that matter in Coup: action -> (challenge?) -> (counter?) ->
//!   (challenge counter?) -> resolution -> (choose loss).
//! - Support multi-player targeting.
//! - Allow Information-Set MCTS by sampling opponent hidden cards each iteration.
//!
//! Notes / limitations:
//! - Tuned for decision making, not perfect rule coverage.
//! - We model at most one challenger per claim (picked stochastically).
//! - We do not rebuild a full engine History stream during rollouts; policies should
//!   primarily use beliefs and public info.

use rand::prelude::*;

use crate::bot::{Context, OtherBot};
use crate::{Action, Card};

/// A policy hook used by the simulator during playouts.
///
/// The real game engine calls the bot for decisions during the actual game.
/// Inside MCTS we need a model for how players react in challenge/counter phases.
pub trait PlayoutPolicy {
    /// Decide what action `player` takes on their turn in a simulated context.
    fn decide_turn(&self, player: &str, ctx: &Context) -> Action;

    /// Decide whether `player` blocks/counters `action` declared by `by`.
    fn decide_counter(&self, player: &str, action: &Action, by: &str, ctx: &Context) -> bool;

    /// Decide whether `player` challenges `action` declared by `by`.
    fn decide_challenge_action(
        &self,
        player: &str,
        action: &Action,
        by: &str,
        ctx: &Context,
    ) -> bool;

    /// Decide whether `player` challenges the counter to `action`, made by `by`.
    fn decide_challenge_counter(
        &self,
        player: &str,
        action: &Action,
        by: &str,
        ctx: &Context,
    ) -> bool;

    /// If `player` loses influence, choose which card to lose.
    ///
    /// Returning `None` means "lose a random influence".
    fn choose_influence_to_lose(&self, player: &str, ctx: &Context) -> Option<Card>;
}

#[derive(Clone, Debug)]
pub struct SimPlayer {
    pub name: String,
    pub coins: u8,
    /// Hidden hand within a determinization.
    pub cards: Vec<Card>,
}

#[derive(Clone, Debug)]
pub struct SimState {
    pub root_name: String,
    pub players: Vec<SimPlayer>,
    pub to_move: usize,
    pub discard_pile: Vec<Card>,

    /// Length of the engine's public history at the time of determinization.
    /// (Kept for possible future extensions; unused by default.)
    pub public_history_len: usize,
}

impl SimState {
    /// Create a determinized simulation state by sampling opponents' hidden cards.
    ///
    /// This is called once per MCTS iteration.
    pub fn from_context_determinized<P: PlayoutPolicy>(
        context: &Context,
        _policy: &P,
        rng: &mut ThreadRng,
    ) -> Self {
        // Construct a remaining deck (3 copies of each role).
        let mut deck: Vec<Card> = Vec::with_capacity(15);
        for _ in 0..3 {
            deck.push(Card::Duke);
            deck.push(Card::Assassin);
            deck.push(Card::Captain);
            deck.push(Card::Ambassador);
            deck.push(Card::Contessa);
        }

        // Remove known visible: our hand + discard pile.
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

        // Root (us).
        let mut players = Vec::new();
        players.push(SimPlayer {
            name: context.name.clone(),
            coins: context.coins,
            cards: context.cards.clone(),
        });

        // Opponents: sample hidden cards consistent with their remaining influence count.
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
            public_history_len: context.history.len(),
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.players.iter().filter(|p| !p.cards.is_empty()).count() <= 1
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

    /// Reward from the root player's perspective.
    pub fn reward_for_root(&self) -> f32 {
        if let Some(w) = self.winner_name() {
            return if w == self.root_name { 1.0 } else { 0.0 };
        }

        // Non-terminal heuristic at depth cutoff.
        let root = self
            .players
            .iter()
            .find(|p| p.name == self.root_name)
            .expect("root exists");

        let root_inf = root.cards.len() as f32;
        let root_coin = root.coins as f32;

        let mut opp_inf_sum = 0.0f32;
        let mut opp_best_coin: f32 = 0.0;

        for p in &self.players {
            if p.name == self.root_name || p.cards.is_empty() {
                continue;
            }
            opp_inf_sum += p.cards.len() as f32;
            opp_best_coin = opp_best_coin.max(p.coins as f32);
        }

        let inf_term = 0.12 * (root_inf - (opp_inf_sum / 2.0));
        let coin_term = 0.03 * root_coin;
        let opp_tempo_threat = 0.04 * (opp_best_coin / 7.0);
        let rich_penalty = 0.02 * ((root_coin - 8.0).max(0.0) / 5.0);

        (0.5 + inf_term + coin_term - opp_tempo_threat - rich_penalty).clamp(0.0, 1.0)
    }

    fn current_player_idx(&self) -> usize {
        self.to_move
    }

    fn current_player(&self) -> &SimPlayer {
        &self.players[self.to_move]
    }

    fn opponent_indices_of(&self, player_idx: usize) -> impl Iterator<Item = usize> + '_ {
        (0..self.players.len()).filter(move |i| {
            *i != player_idx && !self.players[*i].cards.is_empty()
        })
    }

    fn find_idx_by_name(&self, name: &str) -> Option<usize> {
        self.players.iter().position(|p| p.name == name)
    }

    fn next_alive_after(&self, mut idx: usize) -> usize {
        if self.is_terminal() {
            return idx;
        }
        for _ in 0..self.players.len() {
            idx = (idx + 1) % self.players.len();
            if !self.players[idx].cards.is_empty() {
                return idx;
            }
        }
        idx
    }

    /// Public-ish context for the given player index.
    ///
    /// We keep `history` empty during rollouts; the policy should primarily use beliefs.
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
            history: vec![],
            score: vec![],
        }
    }

    /// Legal actions for the root player's on_turn. (Targets expanded.)
    pub fn legal_root_actions(&self) -> Vec<Action> {
        self.legal_actions_for(self.current_player_idx())
    }

    pub fn legal_actions_for(&self, player_idx: usize) -> Vec<Action> {
        let me = &self.players[player_idx];
        if me.cards.is_empty() {
            return vec![];
        }

        // Forced coup rule (common): only coups if >= 10.
        if me.coins >= 10 {
            return self
                .opponent_indices_of(player_idx)
                .map(|ti| Action::Coup(self.players[ti].name.clone()))
                .collect();
        }

        let mut a = vec![Action::Income, Action::ForeignAid, Action::Tax, Action::Swapping];

        // Targeted actions.
        for ti in self.opponent_indices_of(player_idx) {
            let t = self.players[ti].name.clone();
            a.push(Action::Stealing(t.clone()));
            if me.coins >= 3 {
                a.push(Action::Assassination(t.clone()));
            }
            if me.coins >= 7 {
                a.push(Action::Coup(t));
            }
        }

        a
    }

    /// Run a complete playout from a chosen root action.
    pub fn playout_from_root_action<P: PlayoutPolicy>(
        &mut self,
        first_action: &Action,
        policy: &P,
        rng: &mut ThreadRng,
        max_depth: usize,
    ) -> f32 {
        self.apply_declared_action(first_action, policy, rng);

        for _ in 0..max_depth {
            if self.is_terminal() {
                break;
            }

            // Advance to the next alive player (safety).
            if self.current_player().cards.is_empty() {
                self.to_move = self.next_alive_after(self.to_move);
                continue;
            }

            let actor_idx = self.to_move;
            let actor_name = self.players[actor_idx].name.clone();
            let ctx = self.as_context_for_player(actor_idx);
            let mut declared = policy.decide_turn(&actor_name, &ctx);

            // Clamp to legal actions.
            let legal = self.legal_actions_for(actor_idx);
            if !legal.iter().any(|a| a == &declared) {
                declared = legal[rng.gen_range(0..legal.len())].clone();
            }

            self.apply_declared_action(&declared, policy, rng);
        }

        self.reward_for_root()
    }

    /// Apply a declared action, including challenge/counter phases, then advance turn.
    fn apply_declared_action<P: PlayoutPolicy>(
        &mut self,
        action: &Action,
        policy: &P,
        rng: &mut ThreadRng,
    ) {
        if self.is_terminal() {
            return;
        }
        let actor_idx = self.to_move;
        if self.players[actor_idx].cards.is_empty() {
            self.to_move = self.next_alive_after(self.to_move);
            return;
        }

        // 1) Challenge action (if challengeable)
        // 2) Counter/block (if blockable)
        // 3) Challenge counter (if countered)
        // 4) Apply effect

        let actor_name = self.players[actor_idx].name.clone();

        // (1) Action challenge
        if let Some(required_role) = required_role_for_action(action) {
            if let Some(challenger_idx) =
                self.pick_challenger_for_action(action, &actor_name, policy, rng)
            {
                let ok = self.player_has_role(actor_idx, required_role);
                if ok {
                    // Challenger loses influence.
                    self.lose_influence(challenger_idx, policy, rng);
                } else {
                    // Actor lied: actor loses influence and action fails.
                    self.lose_influence(actor_idx, policy, rng);
                    self.to_move = self.next_alive_after(self.to_move);
                    return;
                }
            }
        }

        // (2) Counter/block
        let mut blocked_by: Option<usize> = None;
        if is_blockable(action) {
            // Foreign aid: anyone may block.
            // Steal/assassinate: target may block.
            let candidates: Vec<usize> = match action {
                Action::ForeignAid => self.opponent_indices_of(actor_idx).collect(),
                Action::Stealing(t) | Action::Assassination(t) => {
                    self.find_idx_by_name(t).into_iter().collect()
                }
                _ => vec![],
            };

            for bi in candidates {
                if self.players[bi].cards.is_empty() {
                    continue;
                }
                let bname = self.players[bi].name.clone();
                let bctx = self.as_context_for_player(bi);
                if policy.decide_counter(&bname, action, &actor_name, &bctx) {
                    blocked_by = Some(bi);
                    break;
                }
            }
        }

        // (3) Challenge counter
        if let Some(blocker_idx) = blocked_by {
            let blocker_name = self.players[blocker_idx].name.clone();
            let act_ctx = self.as_context_for_player(actor_idx);
            let should_challenge =
                policy.decide_challenge_counter(&actor_name, action, &blocker_name, &act_ctx);

            if should_challenge {
                let ok = self.block_is_truthful(blocker_idx, action);
                if ok {
                    // Challenger loses.
                    self.lose_influence(actor_idx, policy, rng);
                    self.to_move = self.next_alive_after(self.to_move);
                    return;
                } else {
                    // Blocker lied: blocker loses and action proceeds.
                    self.lose_influence(blocker_idx, policy, rng);
                    // proceed to apply effect
                }
            } else {
                // Not challenged: action is blocked.
                self.to_move = self.next_alive_after(self.to_move);
                return;
            }
        }

        // (4) Apply effect
        self.apply_action_effect(action, rng);
        self.to_move = self.next_alive_after(self.to_move);
    }

    fn apply_action_effect(&mut self, action: &Action, rng: &mut ThreadRng) {
        let actor_idx = self.to_move;
        match action {
            Action::Income => {
                self.players[actor_idx].coins = self.players[actor_idx].coins.saturating_add(1);
            }
            Action::ForeignAid => {
                self.players[actor_idx].coins = self.players[actor_idx].coins.saturating_add(2);
            }
            Action::Tax => {
                self.players[actor_idx].coins = self.players[actor_idx].coins.saturating_add(3);
            }
            Action::Swapping => {
                // Simplified: randomise the player's hand consistent with public info.
                self.randomise_hand(actor_idx, rng);
            }
            Action::Stealing(target_name) => {
                if let Some(ti) = self.find_idx_by_name(target_name) {
                    let steal_amt = self.players[ti].coins.min(2);
                    self.players[ti].coins -= steal_amt;
                    self.players[actor_idx].coins =
                        self.players[actor_idx].coins.saturating_add(steal_amt);
                }
            }
            Action::Assassination(target_name) => {
                if self.players[actor_idx].coins >= 3 {
                    self.players[actor_idx].coins -= 3;
                    if let Some(ti) = self.find_idx_by_name(target_name) {
                        self.lose_influence_random(ti, rng);
                    }
                }
            }
            Action::Coup(target_name) => {
                if self.players[actor_idx].coins >= 7 {
                    self.players[actor_idx].coins -= 7;
                    if let Some(ti) = self.find_idx_by_name(target_name) {
                        self.lose_influence_random(ti, rng);
                    }
                }
            }
        }
    }

    fn pick_challenger_for_action<P: PlayoutPolicy>(
        &self,
        action: &Action,
        actor_name: &str,
        policy: &P,
        rng: &mut ThreadRng,
    ) -> Option<usize> {
        let mut pool = Vec::new();
        for oi in self.opponent_indices_of(self.to_move) {
            let pname = &self.players[oi].name;
            let ctx = self.as_context_for_player(oi);
            if policy.decide_challenge_action(pname, action, actor_name, &ctx) {
                pool.push(oi);
            }
        }
        if pool.is_empty() {
            None
        } else {
            Some(pool[rng.gen_range(0..pool.len())])
        }
    }

    fn player_has_role(&self, player_idx: usize, role: Card) -> bool {
        self.players[player_idx].cards.iter().any(|c| *c == role)
    }

    fn block_is_truthful(&self, blocker_idx: usize, action: &Action) -> bool {
        match action {
            Action::ForeignAid => self.player_has_role(blocker_idx, Card::Duke),
            Action::Stealing(_) => {
                self.player_has_role(blocker_idx, Card::Captain)
                    || self.player_has_role(blocker_idx, Card::Ambassador)
            }
            Action::Assassination(_) => self.player_has_role(blocker_idx, Card::Contessa),
            _ => false,
        }
    }

    fn lose_influence<P: PlayoutPolicy>(
        &mut self,
        player_idx: usize,
        policy: &P,
        rng: &mut ThreadRng,
    ) {
        if self.players[player_idx].cards.is_empty() {
            return;
        }
        let pname = self.players[player_idx].name.clone();
        let ctx = self.as_context_for_player(player_idx);

        if let Some(card) = policy.choose_influence_to_lose(&pname, &ctx) {
            if let Some(pos) = self.players[player_idx].cards.iter().position(|c| *c == card) {
                let lost = self.players[player_idx].cards.swap_remove(pos);
                self.discard_pile.push(lost);
                return;
            }
        }

        self.lose_influence_random(player_idx, rng);
    }

    fn lose_influence_random(&mut self, player_idx: usize, rng: &mut ThreadRng) {
        if self.players[player_idx].cards.is_empty() {
            return;
        }
        let i = rng.gen_range(0..self.players[player_idx].cards.len());
        let lost = self.players[player_idx].cards.swap_remove(i);
        self.discard_pile.push(lost);
    }

    fn randomise_hand(&mut self, player_idx: usize, rng: &mut ThreadRng) {
        // IMPORTANT: Card doesn't derive Hash in your project, so do NOT use HashMap<Card,...>.
        // Use fixed array counts for the 5 roles instead.

        fn idx(c: Card) -> usize {
            match c {
                Card::Duke => 0,
                Card::Assassin => 1,
                Card::Captain => 2,
                Card::Ambassador => 3,
                Card::Contessa => 4,
            }
        }
        fn from_idx(i: usize) -> Card {
            match i {
                0 => Card::Duke,
                1 => Card::Assassin,
                2 => Card::Captain,
                3 => Card::Ambassador,
                _ => Card::Contessa,
            }
        }

        // 3 copies of each role.
        let mut counts = [3i32; 5];

        // Remove discards and all currently-held cards from the pool.
        for c in &self.discard_pile {
            counts[idx(*c)] -= 1;
        }
        for p in &self.players {
            for c in &p.cards {
                counts[idx(*c)] -= 1;
            }
        }

        // Build remaining deck.
        let mut deck: Vec<Card> = Vec::new();
        for i in 0..5 {
            for _ in 0..counts[i].max(0) {
                deck.push(from_idx(i));
            }
        }
        deck.shuffle(rng);

        let need = self.players[player_idx].cards.len();
        self.players[player_idx].cards.clear();
        for _ in 0..need {
            if let Some(c) = deck.pop() {
                self.players[player_idx].cards.push(c);
            }
        }
    }
}

fn required_role_for_action(action: &Action) -> Option<Card> {
    match action {
        Action::Tax => Some(Card::Duke),
        Action::Stealing(_) => Some(Card::Captain),
        Action::Assassination(_) => Some(Card::Assassin),
        Action::Swapping => Some(Card::Ambassador),
        _ => None,
    }
}

fn is_blockable(action: &Action) -> bool {
    matches!(
        action,
        Action::ForeignAid | Action::Stealing(_) | Action::Assassination(_)
    )
}
