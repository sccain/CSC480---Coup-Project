// duel_brain.rs
//! A lightweight belief + opponent modelling brain.
//!
//! This started life as a 1v1 "DuelBrain". For the MCTS bot we extend it to support
//! multi-player, and to act as a **playout policy** for simulated challenge/counter
//! rounds.
//!
//! What this brain provides:
//! - Per-opponent claim counts (credibility model)
//! - Simple challenge/counter decisions based on (a) deck scarcity, (b) credibility,
//!   (c) stakes/tempo
//! - A turn policy that chooses targets and actions in a multi-player setting
//!
//! It is intentionally cheap: in MCTS we call it *a lot*.

use std::collections::HashMap;

use crate::{
    bot::{Context, OtherBot},
    Action, Card, History,
};

use crate::mcts::sim_state::PlayoutPolicy;

const N_CARDS: usize = 5;
const RECENT_BLOCK_WINDOW: usize = 10;      // how far back (history entries) to look for "I got blocked"
const SWAP_COOLDOWN_ACTIONS: usize = 8;      // how far back to prevent repeated Swapping

fn card_idx(c: Card) -> usize {
    match c {
        Card::Duke => 0,
        Card::Assassin => 1,
        Card::Captain => 2,
        Card::Ambassador => 3,
        Card::Contessa => 4,
    }
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

#[derive(Clone, Default)]
struct OppStats {
    // Credibility model: how often we've seen them claim each role (action or counter).
    claims: [u32; N_CARDS],
    // Behavioural model: how often they challenge.
    challenges: u32,
    opportunities_to_challenge: u32,
}

#[derive(Clone, Default)]
pub struct DuelBrain {
    seen_history_len: usize,
    // Per-opponent statistics
    opp: HashMap<String, OppStats>,
}

impl DuelBrain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.seen_history_len = 0;
        self.opp.clear();
    }

    /// Update opponent models from the public game history.
    pub fn update_from_history(&mut self, context: &Context) {
        if context.history.is_empty() {
            // New game.
            self.reset();
            return;
        }
        if self.seen_history_len >= context.history.len() {
            return;
        }

        for h in &context.history[self.seen_history_len..] {
            match h {
                // Action claims
                History::ActionTax { by } => {
                    self.opp.entry(by.clone()).or_default().claims[card_idx(Card::Duke)] += 1;
                }
                History::ActionAssassination { by, .. } => {
                    self.opp.entry(by.clone()).or_default().claims[card_idx(Card::Assassin)] += 1;
                }
                History::ActionStealing { by, .. } => {
                    self.opp.entry(by.clone()).or_default().claims[card_idx(Card::Captain)] += 1;
                }
                History::ActionSwapping { by } => {
                    self.opp
                        .entry(by.clone())
                        .or_default()
                        .claims[card_idx(Card::Ambassador)] += 1;
                }

                // Counter claims
                History::CounterForeignAid { by, .. } => {
                    self.opp.entry(by.clone()).or_default().claims[card_idx(Card::Duke)] += 1;
                }
                History::CounterAssassination { by, .. } => {
                    self.opp
                        .entry(by.clone())
                        .or_default()
                        .claims[card_idx(Card::Contessa)] += 1;
                }
                History::CounterStealing { by, .. } => {
                    let s = self.opp.entry(by.clone()).or_default();
                    s.claims[card_idx(Card::Captain)] += 1;
                    s.claims[card_idx(Card::Ambassador)] += 1;
                }

                _ => {}
            }
        }

        self.seen_history_len = context.history.len();
    }

    fn opponent<'a>(&self, context: &'a Context, name: &str) -> Option<&'a OtherBot> {
        context.playing_bots.iter().find(|b| b.name == name)
    }

    fn visible_count(context: &Context, card: Card) -> usize {
        let hand = context.cards.iter().filter(|c| **c == card).count();
        let discard = context.discard_pile.iter().filter(|c| **c == card).count();
        hand + discard
    }

    fn remaining_copies(context: &Context, card: Card) -> i32 {
        (3 - Self::visible_count(context, card) as i32).max(0)
    }

    fn hidden_total(context: &Context) -> i32 {
        let visible = context.cards.len() as i32 + context.discard_pile.len() as i32;
        (15 - visible).max(0)
    }

    fn n_choose_k(n: i32, k: i32) -> f64 {
        if k < 0 || k > n {
            return 0.0;
        }
        let k = k.min(n - k);
        let mut num = 1.0;
        let mut den = 1.0;
        for i in 1..=k {
            num *= (n - (k - i)) as f64;
            den *= i as f64;
        }
        num / den
    }

    /// Baseline probability an opponent has `card`, based only on deck composition.
    fn p_has_base(context: &Context, opp_cards: i32, card: Card) -> f64 {
        let n = Self::hidden_total(context);
        let k = Self::remaining_copies(context, card);
        if k == 0 || opp_cards <= 0 || n <= 0 {
            return 0.0;
        }
        let h = opp_cards.min(n);
        1.0 - (Self::n_choose_k(n - k, h) / Self::n_choose_k(n, h))
    }

    /// Belief that `opp_name` has `card`, with a credibility adjustment.
    fn p_opponent_has(&self, context: &Context, opp_name: &str, opp_cards: i32, card: Card) -> f64 {
        let base = Self::p_has_base(context, opp_cards, card).clamp(0.0001, 0.9999);

        // Credibility boost based on repeated claims.
        let claims = self
            .opp
            .get(opp_name)
            .map(|s| s.claims[card_idx(card)] as f64)
            .unwrap_or(0.0);

        // Convert to odds, scale, convert back.
        let mut odds = base / (1.0 - base);
        odds *= (0.30 * claims).exp();
        (odds / (1.0 + odds)).clamp(0.001, 0.999)
    }

    /// Estimated probability `player_name` challenges a claim of `claimed_role` by someone else.
    fn p_player_challenges_claim(
        &self,
        context: &Context,
        player_name: &str,
        claimed_role: Card,
        stake: f64,
        actor_cards: i32,
    ) -> f64 {
        // Scarcity: rarer roles are more likely to be challenged.
        let remaining = Self::remaining_copies(context, claimed_role) as f64;
        let scarcity = 1.0 - (remaining / 3.0);

        // If the role literally cannot exist, challenge is almost certain.
        if remaining <= 0.0 {
            return 0.99;
        }

        // If *we* likely have the role, we may be less incentivized to challenge (we can block, etc.).
        let my_cards = context.cards.len() as i32;
        let p_i_have = Self::p_has_base(context, my_cards, claimed_role);

        // Actor credibility: if actor has claimed this role often, challenge less.
        // We don't know the actor name here; so we use a generic actor_cards pressure term.
        let actor_pressure = (actor_cards as f64 - 1.0) * -0.12;

        // Player aggression: based on observed challenge frequency.
        let aggress = self
            .opp
            .get(player_name)
            .map(|s| {
                let denom = (s.opportunities_to_challenge.max(1)) as f64;
                (s.challenges as f64 / denom).clamp(0.0, 1.0)
            })
            .unwrap_or(0.25);

        let inf_adv = (context.cards.len() as f64) - 1.0;
        let x =
            -0.70 + 1.40 * scarcity + 0.35 * (stake - 1.0) + 0.15 * aggress + actor_pressure
                - 0.30 * (p_i_have - 0.5)
                + 0.05 * inf_adv;

        sigmoid(x).clamp(0.01, 0.99)
    }

    fn choose_best_coup_target(&self, context: &Context) -> Option<String> {
        let mut best: Option<(&OtherBot, f64)> = None;
        for b in &context.playing_bots {
            if b.name == context.name || b.cards == 0 {
                continue;
            }
            // Target score: high influence + high coins.
            let score = (b.cards as f64) * 1.3 + (b.coins as f64) * 0.25;
            if best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some((b, score));
            }
        }
        best.map(|(b, _)| b.name.clone())
    }

    fn choose_best_soft_target(&self, context: &Context) -> Option<String> {
        let mut best: Option<(&OtherBot, f64)> = None;
        for b in &context.playing_bots {
            if b.name == context.name || b.cards == 0 {
                continue;
            }
            // Prefer low influence, high coins (easier elimination / big steal value).
            let score = (7.0 - b.cards as f64) * 1.1 + (b.coins as f64) * 0.35;
            if best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some((b, score));
            }
        }
        best.map(|(b, _)| b.name.clone())
    }

    fn should_block_foreign_aid(&self, context: &Context, by: &str) -> bool {
        // Truthfully block if we have Duke.
        if context.cards.contains(&Card::Duke) {
            return true;
        }

        // If Duke cannot exist, don't bluff.
        if Self::remaining_copies(context, Card::Duke) == 0 {
            return false;
        }

        // Block more if actor is close to coup tempo.
        let actor = self.opponent(context, by);
        let actor_coins = actor.map(|a| a.coins as i32).unwrap_or(0);
        let near_coup = actor_coins >= 6;

        // If we are ahead, we can accept FA sometimes.
        let my_inf = context.cards.len() as i32;
        let my_ahead = actor.map(|a| my_inf > a.cards as i32).unwrap_or(false);

        // Default bluff-block rate.
        if near_coup {
            !my_ahead
        } else {
            false
        }
    }

    fn should_block_steal(&self, context: &Context, by: &str) -> bool {
        if context.cards.contains(&Card::Captain) || context.cards.contains(&Card::Ambassador) {
            return true;
        }
        // If both roles cannot exist, don't bluff.
        if Self::remaining_copies(context, Card::Captain) == 0 && Self::remaining_copies(context, Card::Ambassador) == 0
        {
            return false;
        }
        // Bluff-block more if we're low on coins or the thief is rich.
        let thief = self.opponent(context, by);
        let thief_coins = thief.map(|t| t.coins).unwrap_or(0);
        (context.coins <= 2 && thief_coins >= 3) || thief_coins >= 6
    }

    fn should_block_assassination(&self, context: &Context, by: &str) -> bool {
        if context.cards.contains(&Card::Contessa) {
            return true;
        }
        if Self::remaining_copies(context, Card::Contessa) == 0 {
            return false;
        }
        // With 1 influence left, blocking is extremely valuable.
        if context.cards.len() <= 1 {
            return true;
        }
        // Bluff-block if the assassin is rich or has pressured before.
        let a = self.opponent(context, by);
        a.map(|a| a.coins >= 4).unwrap_or(true)
    }

    fn should_challenge_action_inner(&mut self, context: &Context, action: &Action, by: &str) -> bool {
        let actor = self.opponent(context, by);
        let Some(actor) = actor else { return false; };

        let (role, reward_delta, stake) = match action {
            Action::Tax => (Card::Duke, 1.2, 1.05),
            Action::Swapping => (Card::Ambassador, 0.7, 1.00),
            Action::Stealing(target) if target == &context.name => (Card::Captain, 1.8, 1.15),
            Action::Assassination(target) if target == &context.name => (Card::Assassin, 3.0, 1.45),
            Action::Stealing(_) => (Card::Captain, 0.8, 1.05),
            Action::Assassination(_) => (Card::Assassin, 1.4, 1.15),
            _ => return false,
        };

        if Self::remaining_copies(context, role) == 0 {
            return true;
        }

        // Belief actor has role.
        let p_has = self.p_opponent_has(context, &actor.name, actor.cards as i32, role);
        let p_bluff = 1.0 - p_has;

        let risk_loss = if context.cards.len() <= 1 { 2.0 } else { 1.0 };
        let ev = p_bluff * reward_delta - (1.0 - p_bluff) * risk_loss;

        // Update opportunity count.
        self.opp.entry(actor.name.clone()).or_default().opportunities_to_challenge += 1;

        ev > (0.85 + 0.25 * (stake - 1.0))
    }

    fn should_challenge_counter_inner(&mut self, context: &Context, action: &Action, by: &str) -> bool {
        let blocker = self.opponent(context, by);
        let Some(blocker) = blocker else { return false; };

        let (roles, reward_delta, stake) = match action {
            Action::ForeignAid => (vec![Card::Duke], 1.1, 1.05),
            Action::Stealing(_) => (vec![Card::Captain, Card::Ambassador], 1.5, 1.10),
            Action::Assassination(_) => (vec![Card::Contessa], 2.2, 1.30),
            _ => return false,
        };

        let mut p_has_block: f64 = 0.0;
        for r in roles {
            if Self::remaining_copies(context, r) == 0 {
                continue;
            }
            p_has_block = p_has_block.max(self.p_opponent_has(context, &blocker.name, blocker.cards as i32, r));
        }
        let p_bluff = 1.0 - p_has_block;

        let risk_loss = if context.cards.len() <= 1 { 2.0 } else { 1.0 };
        let ev = p_bluff * reward_delta - (1.0 - p_bluff) * risk_loss;

        self.opp.entry(blocker.name.clone()).or_default().opportunities_to_challenge += 1;

        ev > (0.85 + 0.30 * (stake - 1.0))
    }

    /// Decide whether to counter/block an opponent action.
    pub fn decide_counter(&mut self, action: &Action, by: &str, context: &Context) -> bool {
        self.update_from_history(context);
        match action {
            Action::ForeignAid => self.should_block_foreign_aid(context, by),
            Action::Stealing(target) if target == &context.name => self.should_block_steal(context, by),
            Action::Assassination(target) if target == &context.name => self.should_block_assassination(context, by),
            _ => false,
        }
    }

    /// Decide whether to challenge an opponent's action claim.
    pub fn decide_challenge_action(&mut self, action: &Action, by: &str, context: &Context) -> bool {
        self.update_from_history(context);
        self.should_challenge_action_inner(context, action, by)
    }

    /// Decide whether to challenge an opponent's counter claim.
    pub fn decide_challenge_counter(&mut self, action: &Action, by: &str, context: &Context) -> bool {
        self.update_from_history(context);
        self.should_challenge_counter_inner(context, action, by)
    }

    fn recently_blocked_same_action(&self, context: &Context, proposed: &Action) -> bool {
        let me = &context.name;
        let h = &context.history;
        if h.len() < 2 {
            return false;
        }

        let start = h.len().saturating_sub(RECENT_BLOCK_WINDOW);

        // Robust anti-loop heuristic:
        // - The history may contain an *intervening challenge* entry between an
        //   action declaration and the eventual counter.
        // - Therefore we look for a matching action by us, then scan a few entries
        //   ahead for a matching counter that targets us.
        // - If we immediately counter-challenged, we do NOT treat it as a "blocked
        //   loop" (even if the challenge fails).
        const LOOKAHEAD: usize = 4;

        for i in (start..h.len()).rev() {
            // Find the most recent matching action by us.
            let matches_action = match (proposed, &h[i]) {
                (Action::ForeignAid, History::ActionForeignAid { by }) => by == me,
                (Action::Stealing(t2), History::ActionStealing { by, target: t1 }) => {
                    by == me && t1 == t2
                }
                (Action::Assassination(t2), History::ActionAssassination { by, target: t1 }) => {
                    by == me && t1 == t2
                }
                (Action::Swapping, History::ActionSwapping { by }) => by == me,
                _ => false,
            };
            if !matches_action {
                continue;
            }

            let end = (i + 1 + LOOKAHEAD).min(h.len() - 1);

            // Special-case Swapping: there is no counter; it is stopped by an action-challenge.
            if matches!(proposed, Action::Swapping) {
                for k in (i + 1)..=end {
                    if matches!(&h[k], History::ChallengeAmbassador { target, .. } if target == me)
                    {
                        return true;
                    }
                }
                return false;
            }

            for k in (i + 1)..=end {
                // Foreign Aid blocked
                if matches!(
                    (proposed, &h[k]),
                    (Action::ForeignAid, History::CounterForeignAid { target, .. }) if target == me
                ) {
                    // If the very next entry is our counter-challenge, don't treat it as a block-loop.
                    if k + 1 < h.len()
                        && matches!(
                            (&h[k + 1], proposed),
                            (History::CounterChallengeDuke { by, .. }, Action::ForeignAid) if by == me
                        )
                    {
                        return false;
                    }
                    return true;
                }

                // Steal blocked
                if matches!(
                    (proposed, &h[k]),
                    (Action::Stealing(_), History::CounterStealing { target, .. }) if target == me
                ) {
                    if k + 1 < h.len()
                        && matches!(
                            (&h[k + 1], proposed),
                            (History::CounterChallengeCaptainAmbassedor { by, .. }, Action::Stealing(_)) if by == me
                        )
                    {
                        return false;
                    }
                    return true;
                }

                // Assassination blocked
                if matches!(
                    (proposed, &h[k]),
                    (Action::Assassination(_), History::CounterAssassination { target, .. }) if target == me
                ) {
                    if k + 1 < h.len()
                        && matches!(
                            (&h[k + 1], proposed),
                            (History::CounterChallengeContessa { by, .. }, Action::Assassination(_)) if by == me
                        )
                    {
                        return false;
                    }
                    return true;
                }

                // If we hit our next action, stop looking; we've passed the resolution window.
                if matches!(&h[k],
                    History::ActionIncome { by }
                        | History::ActionForeignAid { by }
                        | History::ActionTax { by }
                        | History::ActionSwapping { by }
                        | History::ActionStealing { by, .. }
                        | History::ActionAssassination { by, .. }
                        | History::ActionCoup { by, .. }
                    if by == me)
                {
                    break;
                }
            }

            // Found a recent attempt but not a matching counter/challenge.
            return false;
        }

        false
    }

    fn recently_swapped(&self, context: &Context) -> bool {
        let me = &context.name;
        let h = &context.history;
        let start = h.len().saturating_sub(SWAP_COOLDOWN_ACTIONS);
        h[start..]
            .iter()
            .any(|e| matches!(e, History::ActionSwapping { by } if by == me))
    }

    /// Multi-player turn policy.
    pub fn decide_turn(&mut self, context: &Context) -> Action {
        self.update_from_history(context);

        // Forced coup if >= 10 coins.
        if context.coins >= 10 {
            if let Some(t) = self.choose_best_coup_target(context) {
                return Action::Coup(t);
            }
            return Action::Income;
        }

        // Coup if possible (good default in Coup).
        if context.coins >= 7 {
            if let Some(t) = self.choose_best_coup_target(context) {
                return Action::Coup(t);
            }
        }

        // Assassinate if we have assassin or if bluff seems safe.
        if context.coins >= 3 {
            if let Some(t) = self.choose_best_soft_target(context) {
                // Don't assassinate if target very likely has contessa.
                let p_contessa = self
                    .opponent(context, &t)
                    .map(|ob| self.p_opponent_has(context, &t, ob.cards as i32, Card::Contessa))
                    .unwrap_or(0.35);
                let has_assassin = context.cards.contains(&Card::Assassin);
                if has_assassin || p_contessa < 0.55 {
                    let a = Action::Assassination(t.clone());
                    if !self.recently_blocked_same_action(context, &a) {
                        return a;
                    }
                    // Otherwise, don't repeat an assassination that was just blocked.
                }
            }
        }

        // Steal if someone is rich.
        if let Some(t) = self.choose_best_soft_target(context) {
            let coins = self.opponent(context, &t).map(|b| b.coins).unwrap_or(0);
            if coins >= 2 {
                let a = Action::Stealing(t.clone());
                if !self.recently_blocked_same_action(context, &a) {
                    return a;
                }
                // Otherwise, don't repeat a steal that was just blocked.
            }
        }

        // Tax if we have Duke, or if Duke is not extremely scarce.
        if context.cards.contains(&Card::Duke) || Self::remaining_copies(context, Card::Duke) >= 1 {
            return Action::Tax;
        }

        // Swapping is useful to reset beliefs when low influence,
        // but never allow it to spam indefinitely.
        if context.cards.len() <= 1 && !self.recently_swapped(context) {
            let a = Action::Swapping;
            if !self.recently_blocked_same_action(context, &a) {
                return a;
            }
        }

        // Otherwise: safe tempo.
        if Self::remaining_copies(context, Card::Duke) >= 2 && context.coins <= 1 {
            return Action::ForeignAid;
        }
        Action::Income
    }
}

// --- PlayoutPolicy implementation ---

impl PlayoutPolicy for DuelBrain {
    fn decide_turn(&self, player: &str, ctx: &Context) -> Action {
        // Playouts use a cloned brain per rollout in the simulator; but this method
        // takes &self for speed. We therefore use a tiny "stateless" heuristic here.
        //
        // The MCTSBot passes a *mutable* brain to search; the simulator will call the
        // mutable wrappers below when it wants update-from-history effects.
        if player == ctx.name {
            // If we are called with ctx for the same player, fall back to a conservative policy.
        }

        // Simple heuristic: coup if possible, else tax, else income.
        if ctx.coins >= 7 {
            if let Some(t) = ctx
                .playing_bots
                .iter()
                .filter(|b| b.cards > 0)
                .max_by_key(|b| (b.cards, b.coins))
                .map(|b| b.name.clone())
            {
                return Action::Coup(t);
            }
        }
        if ctx.cards.contains(&Card::Duke) {
            return Action::Tax;
        }
        Action::Income
    }

    fn decide_counter(&self, _player: &str, action: &Action, by: &str, ctx: &Context) -> bool {
        // Stateless conservative counter policy.
        match action {
            Action::Stealing(target) if target == &ctx.name => {
                ctx.cards.contains(&Card::Captain) || ctx.cards.contains(&Card::Ambassador)
            }
            Action::Assassination(target) if target == &ctx.name => ctx.cards.contains(&Card::Contessa),
            Action::ForeignAid => ctx.cards.contains(&Card::Duke) && by != ctx.name,
            _ => false,
        }
    }

    fn decide_challenge_action(&self, _player: &str, _action: &Action, _by: &str, _ctx: &Context) -> bool {
        // Stateless, low-challenge in rollouts.
        false
    }

    fn decide_challenge_counter(&self, _player: &str, _action: &Action, _by: &str, _ctx: &Context) -> bool {
        false
    }

    fn choose_influence_to_lose(&self, _player: &str, ctx: &Context) -> Option<Card> {
        // Prefer to lose cards that are currently "less useful".
        // This is a crude ordering.
        for c in [Card::Ambassador, Card::Contessa, Card::Captain, Card::Assassin, Card::Duke] {
            if ctx.cards.contains(&c) {
                return Some(c);
            }
        }
        None
    }
}
