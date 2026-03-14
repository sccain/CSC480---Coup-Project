// simulation for rollout for MCTS bot

use rand::prelude::*;
use std::collections::HashSet;

use crate::bot::{Context, OtherBot};
use crate::{Action, Card, History};

const RECENT_FAILURE_WINDOW: usize = 14;
const POST_ACTION_LOOKAHEAD: usize = 4;
const SWAP_COOLDOWN_ACTIONS: usize = 8;

pub trait PlayoutPolicy {
    fn decide_turn(&self, player: &str, ctx: &Context) -> Action;
    fn decide_counter(&self, player: &str, action: &Action, by: &str, ctx: &Context) -> bool;
    fn decide_challenge_action(
        &self,
        player: &str,
        action: &Action,
        by: &str,
        ctx: &Context,
    ) -> bool;

    fn decide_challenge_counter(
        &self,
        player: &str,
        action: &Action,
        by: &str,
        ctx: &Context,
    ) -> bool;

    fn choose_influence_to_lose(&self, player: &str, ctx: &Context) -> Option<Card>;
}

#[derive(Clone, Debug)]
pub struct SimPlayer {
    pub name: String,
    pub coins: u8,
    pub cards: Vec<Card>,
}

#[derive(Clone, Debug)]
pub struct SimState {
    pub root_name: String,
    pub players: Vec<SimPlayer>,
    pub to_move: usize,

    pub deck: Vec<Card>,
    pub discard_pile: Vec<Card>,
    pub history: Vec<History>,
    pub public_history_len: usize,

    root_tempo_waste: f32,

    loop_penalty: f32,
}

impl SimState {
    //Create a determinized simulation state by sampling opponents' hidden cards.
    // This is called once per MCTS iteration.
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

        // Remove cards not possible for the opponent to have
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

        for ob in &context.playing_bots {
            if ob.name == context.name {
                continue;
            }
            let need = ob.cards as usize;
            let mut opp_cards = Vec::with_capacity(need);
            for p in 0..need {
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
            deck,
            discard_pile: context.discard_pile.clone(),
            history: context.history.clone(),
            public_history_len: context.history.len(),
            root_tempo_waste: 0.0,
            loop_penalty: 0.0,
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

    // Reward from the root
    pub fn reward_for_root(&self) -> f32 {
        if let Some(w) = self.winner_name() {
            return if w == self.root_name { 1.0 } else { 0.0 };
        }

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

        // Penalize wasted moves
        let tempo_penalty = 0.04 * self.root_tempo_waste;
        let loop_penalty = 0.15 * self.loop_penalty;

        (0.5 + inf_term + coin_term - opp_tempo_threat - rich_penalty - tempo_penalty - loop_penalty)
            .clamp(0.0, 1.0)
    }

    fn current_player_idx(&self) -> usize {
        self.to_move
    }

    fn current_player(&self) -> &SimPlayer {
        &self.players[self.to_move]
    }

    fn opponent_indices_of(&self, player_idx: usize) -> impl Iterator<Item = usize> + '_ {
        (0..self.players.len())
            .filter(move |i| *i != player_idx && !self.players[*i].cards.is_empty())
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

    // Context for the given player index.
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
            history: self.history.clone(),
            score: vec![],
        }
    }

    // Legal actions for the root player's on_turn
    pub fn legal_root_actions(&self) -> Vec<Action> {
        self.legal_actions_for(self.current_player_idx())
    }

    pub fn legal_actions_for(&self, player_idx: usize) -> Vec<Action> {
        let me = &self.players[player_idx];
        if me.cards.is_empty() {
            return vec![];
        }

        // Forced coup rule: only coups if >= 10.
        if me.coins >= 10 {
            return self
                .opponent_indices_of(player_idx)
                .map(|ti| Action::Coup(self.players[ti].name.clone()))
                .collect();
        }

        let mut a = vec![Action::Income, Action::ForeignAid, Action::Tax, Action::Swapping];

        // Targeted actions
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

        // Filter out actions that are very likely to be a waste of a move
        a.into_iter()
            .filter(|act| !self.action_is_on_cooldown(&me.name, act))
            .collect()
    }

    fn action_is_on_cooldown(&self, player_name: &str, action: &Action) -> bool {
        if matches!(action, Action::Coup(_)) {
            return false;
        }

        if matches!(action, Action::Swapping) {
            return self.recently_swapped(player_name);
        }

        // Prevent repeating an action that was just blocked.
        self.recently_blocked(player_name, action)
    }

    fn recently_swapped(&self, player_name: &str) -> bool {
        let h = &self.history;
        let start = h.len().saturating_sub(SWAP_COOLDOWN_ACTIONS);
        h[start..]
            .iter()
            .any(|e| matches!(e, History::ActionSwapping { by } if by == player_name))
    }

    fn recently_blocked(&self, player_name: &str, proposed: &Action) -> bool {
        let h = &self.history;
        if h.len() < 2 {
            return false;
        }

        let start = h.len().saturating_sub(RECENT_FAILURE_WINDOW);

        // Search for the most recent matching action by player, then see if a
        // corresponding counter appears shortly after it.
        for i in (start..h.len()).rev() {
            if !history_is_matching_action(&h[i], player_name, proposed) {
                continue;
            }

            let end = (i + 1 + POST_ACTION_LOOKAHEAD).min(h.len() - 1);
            // Scan forward for a matching counter that targets the actor.
            for k in (i + 1)..=end {
                if let Some(blocker_name) = history_is_matching_counter(&h[k], player_name, proposed)
                {
                    // If the actor immediately challenged the counter, we don't
                    // treat it as a "pure block loop" (even if the challenge fails).
                    if k + 1 < h.len()
                        && history_is_counter_challenge(&h[k + 1], player_name, &blocker_name, proposed)
                    {
                        return false;
                    }
                    return true;
                }

                // If we hit another action by this player, stop; we've moved past the
                // resolution window for the candidate action.
                if matches!(&h[k],
                    History::ActionIncome { by }
                        | History::ActionForeignAid { by }
                        | History::ActionTax { by }
                        | History::ActionSwapping { by }
                        | History::ActionStealing { by, .. }
                        | History::ActionAssassination { by, .. }
                        | History::ActionCoup { by, .. }
                    if by == player_name)
                {
                    break;
                }
            }

            // Found a recent attempt but didn't find a counter in the resolution window.
            // This isn't the blocked-loop case we're trying to prevent.
            return false;
        }

        false
    }

    pub fn playout_from_root_action<P: PlayoutPolicy>(
        &mut self,
        first_action: &Action,
        policy: &P,
        rng: &mut ThreadRng,
        max_depth: usize,
    ) -> f32 {
        let mut seen: HashSet<Vec<u8>> = HashSet::new();
        seen.insert(self.public_signature());

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

            // Loop detection: if we re-enter the same public-ish state, terminate.
            let sig = self.public_signature();
            if !seen.insert(sig) {
                self.loop_penalty += 1.0;
                break;
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

    fn public_signature(&self) -> Vec<u8> {
        // Very cheap signature capturing the public-ish part of state.
        // (to_move, per-player coins, per-player influence count)
        let mut v = Vec::with_capacity(1 + self.players.len() * 2);
        v.push(self.to_move as u8);
        for p in &self.players {
            v.push(p.coins);
            v.push(p.cards.len() as u8);
        }
        v
    }

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

        let actor_name = self.players[actor_idx].name.clone();
        self.push_action_history(&actor_name, action);


        if let Some(required_role) = required_role_for_action(action) {
            if let Some(challenger_idx) =
                self.pick_challenger_for_action(action, &actor_name, policy, rng)
            {
                let challenger_name = self.players[challenger_idx].name.clone();
                self.push_action_challenge_history(required_role, &challenger_name, &actor_name);

                let ok = self.player_has_role(actor_idx, required_role);
                if ok {
                    // Challenger loses influence.
                    self.lose_influence(challenger_idx, policy, rng);
                    // Actor reveals and replaces the claimed role.
                    self.replace_revealed_card(actor_idx, required_role, rng);
                } else {
                    // Actor lied: actor loses influence and action fails.
                    self.lose_influence(actor_idx, policy, rng);
                    self.to_move = self.next_alive_after(self.to_move);
                    return;
                }
            }
        }

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
                    // Record the counter claim.
                    self.push_counter_history(&bname, &actor_name, action);
                    blocked_by = Some(bi);
                    break;
                }
            }
        }

        if let Some(blocker_idx) = blocked_by {
            let blocker_name = self.players[blocker_idx].name.clone();
            let act_ctx = self.as_context_for_player(actor_idx);
            let should_challenge =
                policy.decide_challenge_counter(&actor_name, action, &blocker_name, &act_ctx);

            if should_challenge {
                self.push_counter_challenge_history(action, &actor_name, &blocker_name);

                let ok = self.block_is_truthful(blocker_idx, action);
                if ok {
                    // Challenger loses.
                    self.lose_influence(actor_idx, policy, rng);

                    // Blocker reveals and replaces (real Coup rule).
                    match action {
                        Action::ForeignAid => self.replace_revealed_card(blocker_idx, Card::Duke, rng),
                        Action::Assassination(_) => {
                            self.replace_revealed_card(blocker_idx, Card::Contessa, rng)
                        }
                        Action::Stealing(_) => {
                            if self.player_has_role(blocker_idx, Card::Captain) {
                                self.replace_revealed_card(blocker_idx, Card::Captain, rng);
                            } else if self.player_has_role(blocker_idx, Card::Ambassador) {
                                self.replace_revealed_card(blocker_idx, Card::Ambassador, rng);
                            }
                        }
                        _ => {}
                    }

                    self.to_move = self.next_alive_after(self.to_move);
                    return;
                } else {
                    // Blocker lied: blocker loses and action proceeds.
                    self.lose_influence(blocker_idx, policy, rng);
                    // proceed to apply effect
                }
            } else {
                // Not challenged: action is blocked.
                if self.players[actor_idx].name == self.root_name {
                    self.root_tempo_waste += 1.0;
                }
                self.to_move = self.next_alive_after(self.to_move);
                return;
            }
        }

        self.apply_action_effect(action, policy, rng);
        self.to_move = self.next_alive_after(self.to_move);
    }

    fn apply_action_effect<P: PlayoutPolicy>(
        &mut self,
        action: &Action,
        _policy: &P,
        rng: &mut ThreadRng,
    ) {
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
                // Exchange (Ambassador): draw 2 from deck, choose 2 to keep, return 2.
                self.exchange_cards(actor_idx, rng);
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

    fn replace_revealed_card(&mut self, player_idx: usize, role: Card, rng: &mut ThreadRng) {
        // Real Coup rule: if you win a challenge, you reveal the card, then shuffle
        // it back into the deck and draw a replacement.
        if self.deck.is_empty() {
            return;
        }
        if let Some(pos) = self.players[player_idx].cards.iter().position(|c| *c == role) {
            // Put revealed role back.
            self.deck.push(role);
            self.deck.shuffle(rng);
            // Draw replacement.
            let drawn = self.deck.pop().unwrap();
            self.players[player_idx].cards[pos] = drawn;
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

    fn exchange_cards(&mut self, player_idx: usize, rng: &mut ThreadRng) {
        if self.players[player_idx].cards.is_empty() {
            return;
        }

        // Draw up to 2 from the deck.
        let mut drawn: Vec<Card> = Vec::with_capacity(2);
        for _ in 0..2 {
            if let Some(c) = self.deck.pop() {
                drawn.push(c);
            }
        }

        // Candidate pool = current hand + drawn.
        let mut pool = self.players[player_idx].cards.clone();
        pool.extend_from_slice(&drawn);

        // Choose best two by a simple heuristic.
        let coins = self.players[player_idx].coins;
        pool.sort_by_key(|c| std::cmp::Reverse(Self::card_exchange_value(*c, coins)));

        let keep_n = self.players[player_idx].cards.len().min(2);
        let keep: Vec<Card> = pool.into_iter().take(keep_n).collect();

        // Return all other cards (including the ones we had but didn't keep and the drawn).
        // We don't track exact identities of returned cards beyond the determinization;
        // shuffling back into the deck is good enough here.
        //
        // Start by returning our previous hand.
        for c in self.players[player_idx].cards.drain(..) {
            self.deck.push(c);
        }
        // Return drawn cards.
        for c in drawn {
            self.deck.push(c);
        }

        // Remove kept cards from deck by swapping out the first matching copies.
        // (We just pushed everything back, so these copies definitely exist.)
        for k in &keep {
            if let Some(i) = self.deck.iter().position(|x| x == k) {
                self.deck.swap_remove(i);
            }
        }

        self.players[player_idx].cards = keep;
        self.deck.shuffle(rng);
    }

    fn card_exchange_value(c: Card, coins: u8) -> i32 {
        // A crude but stable value function to avoid "infinite ambassador".
        // - Duke is universally strong.
        // - Assassin becomes much better once you can afford it.
        // - Captain is good tempo.
        // - Contessa is defensive (medium).
        // - Ambassador is utility (low-ish once you've already exchanged).
        match c {
            Card::Duke => 60,
            Card::Assassin => {
                if coins >= 3 {
                    55
                } else {
                    35
                }
            }
            Card::Captain => 45,
            Card::Contessa => 40,
            Card::Ambassador => 30,
        }
    }

    fn push_action_history(&mut self, by: &str, action: &Action) {
        let h = match action {
            Action::Income => History::ActionIncome { by: by.to_string() },
            Action::ForeignAid => History::ActionForeignAid { by: by.to_string() },
            Action::Tax => History::ActionTax { by: by.to_string() },
            Action::Swapping => History::ActionSwapping { by: by.to_string() },
            Action::Stealing(t) => History::ActionStealing {
                by: by.to_string(),
                target: t.clone(),
            },
            Action::Assassination(t) => History::ActionAssassination {
                by: by.to_string(),
                target: t.clone(),
            },
            Action::Coup(t) => History::ActionCoup {
                by: by.to_string(),
                target: t.clone(),
            },
        };
        self.history.push(h);
    }

    fn push_action_challenge_history(&mut self, claimed: Card, by: &str, target: &str) {
        let h = match claimed {
            Card::Assassin => History::ChallengeAssassin {
                by: by.to_string(),
                target: target.to_string(),
            },
            Card::Ambassador => History::ChallengeAmbassador {
                by: by.to_string(),
                target: target.to_string(),
            },
            Card::Captain => History::ChallengeCaptain {
                by: by.to_string(),
                target: target.to_string(),
            },
            Card::Duke => History::ChallengeDuke {
                by: by.to_string(),
                target: target.to_string(),
            },
            // Contessa isn't an action claim in this simplified model.
            Card::Contessa => History::ChallengeDuke {
                by: by.to_string(),
                target: target.to_string(),
            },
        };
        self.history.push(h);
    }

    fn push_counter_history(&mut self, by: &str, target: &str, action: &Action) {
        let h = match action {
            Action::ForeignAid => History::CounterForeignAid {
                by: by.to_string(),
                target: target.to_string(),
            },
            Action::Stealing(_) => History::CounterStealing {
                by: by.to_string(),
                target: target.to_string(),
            },
            Action::Assassination(_) => History::CounterAssassination {
                by: by.to_string(),
                target: target.to_string(),
            },
            _ => return,
        };
        self.history.push(h);
    }

    fn push_counter_challenge_history(&mut self, action: &Action, by: &str, target: &str) {
        let h = match action {
            Action::ForeignAid => History::CounterChallengeDuke {
                by: by.to_string(),
                target: target.to_string(),
            },
            Action::Stealing(_) => History::CounterChallengeCaptainAmbassedor {
                by: by.to_string(),
                target: target.to_string(),
            },
            Action::Assassination(_) => History::CounterChallengeContessa {
                by: by.to_string(),
                target: target.to_string(),
            },
            _ => return,
        };
        self.history.push(h);
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

fn history_is_matching_action(h: &History, actor: &str, proposed: &Action) -> bool {
    match (proposed, h) {
        (Action::ForeignAid, History::ActionForeignAid { by }) => by == actor,
        (Action::Tax, History::ActionTax { by }) => by == actor,
        (Action::Income, History::ActionIncome { by }) => by == actor,
        (Action::Swapping, History::ActionSwapping { by }) => by == actor,
        (Action::Stealing(t2), History::ActionStealing { by, target: t1 }) => by == actor && t1 == t2,
        (Action::Assassination(t2), History::ActionAssassination { by, target: t1 }) => by == actor && t1 == t2,
        (Action::Coup(t2), History::ActionCoup { by, target: t1 }) => by == actor && t1 == t2,
        _ => false,
    }
}

fn history_is_matching_counter(h: &History, actor: &str, proposed: &Action) -> Option<String> {
    match (proposed, h) {
        (Action::ForeignAid, History::CounterForeignAid { by, target }) if target == actor => {
            Some(by.clone())
        }
        (Action::Stealing(_), History::CounterStealing { by, target }) if target == actor => {
            Some(by.clone())
        }
        (Action::Assassination(_), History::CounterAssassination { by, target })
            if target == actor =>
        {
            Some(by.clone())
        }
        _ => None,
    }
}

fn history_is_counter_challenge(h: &History, actor: &str, blocker: &str, proposed: &Action) -> bool {
    match (proposed, h) {
        (Action::ForeignAid, History::CounterChallengeDuke { by, target }) => {
            by == actor && target == blocker
        }
        (Action::Stealing(_), History::CounterChallengeCaptainAmbassedor { by, target }) => {
            by == actor && target == blocker
        }
        (Action::Assassination(_), History::CounterChallengeContessa { by, target }) => {
            by == actor && target == blocker
        }
        _ => false,
    }
}
