use crate::{
    bot::{Context, OtherBot},
    Action, Card, History,
};

const N_CARDS: usize = 5;

#[derive(Clone, Default)]
pub struct DuelMemory {
    pub seen_history_len: usize,
    pub opp_claims: [u32; N_CARDS],
    pub my_assassination_pending: bool,
    pub assassination_blocked_streak: u32,
}

#[derive(Clone, Default)]
pub struct DuelBrain {
    pub mem: DuelMemory,
}

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

impl DuelBrain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.mem = DuelMemory::default();
    }

    pub fn update_from_history(&mut self, context: &Context) {
        // Reset at start of new game
        if context.history.is_empty() {
            self.reset();
            return;
        }

        if self.mem.seen_history_len >= context.history.len() {
            return;
        }

        // 1v1 opponent name (DuelBot is intended for duels)
        let opp_name = context
            .playing_bots
            .iter()
            .find(|b| b.name != context.name)
            .unwrap()
            .name
            .clone();

        for h in &context.history[self.mem.seen_history_len..] {
            match h {
                History::ActionTax { by } if *by == opp_name => {
                    self.mem.opp_claims[card_idx(Card::Duke)] += 1;
                }
                History::ActionAssassination { by, .. } if *by == opp_name => {
                    self.mem.opp_claims[card_idx(Card::Assassin)] += 1;
                }
                History::ActionStealing { by, .. } if *by == opp_name => {
                    self.mem.opp_claims[card_idx(Card::Captain)] += 1;
                }
                History::ActionSwapping { by } if *by == opp_name => {
                    self.mem.opp_claims[card_idx(Card::Ambassador)] += 1;
                }

                History::CounterForeignAid { by, .. } if *by == opp_name => {
                    self.mem.opp_claims[card_idx(Card::Duke)] += 1;
                }
                History::CounterAssassination { by, .. } if *by == opp_name => {
                    self.mem.opp_claims[card_idx(Card::Contessa)] += 1;

                    if self.mem.my_assassination_pending {
                        self.mem.assassination_blocked_streak =
                            self.mem.assassination_blocked_streak.saturating_add(1);
                        self.mem.my_assassination_pending = false;
                    }
                }
                History::CounterStealing { by, .. } if *by == opp_name => {
                    self.mem.opp_claims[card_idx(Card::Captain)] += 1;
                    self.mem.opp_claims[card_idx(Card::Ambassador)] += 1;
                }

                // Our actions reset certain patterns
                History::ActionAssassination { by, .. } if *by == context.name => {
                    self.mem.my_assassination_pending = true;
                }
                History::ActionTax { by } if *by == context.name => {
                    self.mem.my_assassination_pending = false;
                    self.mem.assassination_blocked_streak = 0;
                }
                History::ActionStealing { by, .. } if *by == context.name => {
                    self.mem.my_assassination_pending = false;
                    self.mem.assassination_blocked_streak = 0;
                }
                History::ActionSwapping { by } if *by == context.name => {
                    self.mem.my_assassination_pending = false;
                    self.mem.assassination_blocked_streak = 0;
                }

                _ => {}
            }
        }

        self.mem.seen_history_len = context.history.len();
    }

    fn opp_claims(&self, card: Card) -> u32 {
        self.mem.opp_claims[card_idx(card)]
    }

    fn assassination_blocked_streak(&self) -> u32 {
        self.mem.assassination_blocked_streak
    }

    fn set_assassination_pending(&mut self, pending: bool) {
        self.mem.my_assassination_pending = pending;
    }

    fn opponent<'a>(&self, context: &'a Context) -> &'a OtherBot {
        context.playing_bots.iter().find(|b| b.name != context.name).unwrap()
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

    fn p_opponent_has(&self, context: &Context, opp_cards: i32, card: Card) -> f64 {
        let n = Self::hidden_total(context);
        let k = Self::remaining_copies(context, card);

        if k == 0 || opp_cards <= 0 || n <= 0 {
            return 0.0;
        }

        let h = opp_cards.min(n);
        let base = 1.0 - (Self::n_choose_k(n - k, h) / Self::n_choose_k(n, h));

        // Credibility boost based on repeated claims
        let claims = self.opp_claims(card) as f64;
        let mut odds = base / (1.0 - base + 1e-9);
        odds *= (0.35 * claims).exp();

        (odds / (1.0 + odds)).clamp(0.001, 0.999)
    }

    fn p_opponent_challenges_claim(&self, context: &Context, claimed_role: Card, stake: f64) -> f64 {
        let opp = self.opponent(context);

        let remaining = Self::remaining_copies(context, claimed_role).max(0) as f64;
        if remaining <= 0.0 {
            return 0.999;
        }

        let scarcity = 1.0 - (remaining / 3.0);
        let p_opp_has_role = self.p_opponent_has(context, opp.cards as i32, claimed_role);
        let cover_effect = (p_opp_has_role - 0.5) * -0.4;

        let inf_adv = (opp.cards as f64) - (context.cards.len() as f64);
        let coin_adv = (opp.coins as f64) - (context.coins as f64);

        let x =
            -0.65
            + 1.55 * scarcity
            + 0.35 * inf_adv
            + 0.12 * coin_adv
            + cover_effect
            + 0.35 * (stake - 1.0);

        sigmoid(x).clamp(0.01, 0.99)
    }

    fn bluff_ev_ok(&self, context: &Context, role: Card, stake: f64, reward_delta: f64) -> bool {
        if Self::remaining_copies(context, role) == 0 {
            return false;
        }

        let p_chal = self.p_opponent_challenges_claim(context, role, stake);
        let p_no = 1.0 - p_chal;

        let risk_loss = if context.cards.len() <= 1 { 2.0 } else { 1.0 };
        let ev = p_no * reward_delta - p_chal * risk_loss;

        ev > 0.10
    }

    fn imminent_coup_loss(&self, context: &Context) -> bool {
        let opp = self.opponent(context);
        context.cards.len() <= 1 && opp.coins >= 7
    }

    fn opponent_coup_threat_next_turn(&self, context: &Context) -> bool {
        let opp = self.opponent(context);
        opp.coins >= 6 || opp.coins >= 4
    }

    fn should_attempt_assassination(&self, context: &Context) -> bool {
        if context.coins < 3 {
            return false;
        }

        let opp = self.opponent(context);
        let streak = self.assassination_blocked_streak();
        let p_contessa = self.p_opponent_has(context, opp.cards as i32, Card::Contessa);

        if streak >= 1 && p_contessa > 0.55 {
            return false;
        }
        if streak >= 2 && p_contessa > 0.40 {
            return false;
        }

        true
    }

    fn should_bluff_block_steal(&self, context: &Context, by: &str) -> bool {
        // If all copies are visible, don't bluff a block that cannot exist.
        let can_claim_captain = Self::remaining_copies(context, Card::Captain) > 0;
        let can_claim_ambassador = Self::remaining_copies(context, Card::Ambassador) > 0;
        if !can_claim_captain && !can_claim_ambassador {
            return false;
        }

        // Find the stealing player (duels: should exist)
        let opp = context.playing_bots.iter().find(|b| b.name == by);

        // If we can't find them (weird state), default to bluff-blocking (safe for duels).
        let Some(opp) = opp else {
            return true;
        };

        // If they have already demonstrated stealing, treat it as a big threat:
        // In your model, opponent Stealing claims increment Captain claim count.
        let steal_claims = self.opp_claims(Card::Captain);

        // If we're behind on coins or they can reach coup tempo soon, block more.
        let coin_gap = (opp.coins as i32) - (context.coins as i32);

        // Estimate how likely they are to challenge *our* block claim.
        // We'll choose the "safer" claim (Captain vs Ambassador) to bluff.
        let p_chal_cap = if can_claim_captain {
            self.p_opponent_challenges_claim(context, Card::Captain, 1.05)
        } else {
            1.0
        };
        let p_chal_amb = if can_claim_ambassador {
            self.p_opponent_challenges_claim(context, Card::Ambassador, 1.05)
        } else {
            1.0
        };

        let best_p_chal = p_chal_cap.min(p_chal_amb);

        // Aggressive policy:
        // - If opponent is already stealing (or ahead), bluff-block unless challenge is extremely likely.
        // - Otherwise still bluff-block fairly often (because repeated steals snowball hard in duels).
        if steal_claims >= 1 || coin_gap >= 1 || opp.coins >= 5 {
            best_p_chal < 0.70
        } else {
            best_p_chal < 0.55
        }
    }

    fn should_bluff_block_assassination(&self, context: &Context, by: &str) -> bool {
        // Only Contessa can block assassination.
        // If all Contessas are visible, don't bluff a block that cannot exist.
        let can_claim_contessa = Self::remaining_copies(context, Card::Contessa) > 0;
        if !can_claim_contessa {
            return false;
        }

        // Find the acting player (duels: should exist)
        let opp = context.playing_bots.iter().find(|b| b.name == by);

        // If we can't find them (weird state), default to bluff-blocking.
        // (Same reasoning as your steal function: in a duel this "shouldn't happen".)
        let Some(opp) = opp else {
            return true;
        };

        // Threat / tempo heuristics:
        // - Assassination is huge in duels (often worth bluff-blocking, especially at 1 influence).
        // - If we are on our last influence, we should block unless they're *very* likely to challenge.
        let one_influence_left = context.cards.len() <= 1;

        // If they are rich, they can keep pressuring; blocks buy time.
        // (Assassination costs 3, so having >= 3 means the threat is "live".)
        let opp_can_assassinate_again_soon = opp.coins >= 3;

        // If they are close to coup tempo, preserving influence matters a lot.
        let opp_near_coup = opp.coins >= 6;

        // Optional: if your model records assassination behavior as Assassin claims,
        // you can use it to treat them as "more credible" (so we bluff-block slightly less).
        // If you don't track this, just remove these two lines and keep credible_assassin = false.
        let assassin_claims = self.opp_claims(Card::Assassin);
        let credible_assassin = assassin_claims >= 1;

        // Estimate how likely they are to challenge our Contessa claim.
        // Use the same calibration factor you used for steal (1.05) unless you have a reason to change it.
        let p_chal = self.p_opponent_challenges_claim(context, Card::Contessa, 1.05);

        // Policy:
        // - If we're at 1 influence: bluff-block unless challenge is extremely likely.
        // - If they are near coup / can keep assassinating: still block fairly aggressively.
        // - If they look credible as Assassin: require lower challenge probability (they're more likely to call you).
        if one_influence_left {
            // Desperation mode: block even if likely to be challenged, but not when it's basically guaranteed.
            p_chal < 0.85
        } else if opp_near_coup || opp_can_assassinate_again_soon {
            if credible_assassin {
                p_chal < 0.60
            } else {
                p_chal < 0.70
            }
        } else {
            // If they're not pressuring much, only bluff-block when we expect relatively low challenge probability.
            if credible_assassin {
                p_chal < 0.50
            } else {
                p_chal < 0.55
            }
        }
    }

    /// Decide whether to counter/block an opponent action.
    /// Return `true` to counter, `false` otherwise.
    ///
    /// Currently we focus on blocking Stealing aggressively (Captain/Ambassador).
    pub fn decide_counter(&mut self, action: &Action, by: &str, context: &Context) -> bool {
        self.update_from_history(context);

        match action {
            Action::Stealing(target) if target == &context.name => {
                if context.cards.contains(&Card::Captain) || context.cards.contains(&Card::Ambassador)
                {
                    return true;
                }
                self.should_bluff_block_steal(context, by)
            }

            Action::Assassination(target) if target == &context.name => {
                if context.cards.contains(&Card::Contessa) {
                    return true;
                }
                self.should_bluff_block_assassination(context, by)
            }

            _ => false,
        }
    }

    /// Main “duel policy” action selection (same as DuelBot’s on_turn).
    pub fn decide_turn(&mut self, context: &Context) -> Action {
        self.update_from_history(context);

        let opp = self.opponent(context);
        let target = opp.name.clone();

        if context.coins >= 7 {
            self.set_assassination_pending(false);
            return Action::Coup(target);
        }

        if self.imminent_coup_loss(context) {
            if opp.cards <= 1
                && context.coins >= 3
                && (context.cards.contains(&Card::Assassin)
                    || self.bluff_ev_ok(context, Card::Assassin, 1.45, 2.5))
                && self.should_attempt_assassination(context)
            {
                self.set_assassination_pending(true);
                return Action::Assassination(target);
            }

            if opp.coins >= 2
                && (context.cards.contains(&Card::Captain)
                    || self.bluff_ev_ok(context, Card::Captain, 1.20, 2.0))
            {
                self.set_assassination_pending(false);
                return Action::Stealing(opp.name.clone());
            }

            if context.coins >= 3
                && self.bluff_ev_ok(context, Card::Assassin, 1.55, 2.2)
                && self.should_attempt_assassination(context)
            {
                self.set_assassination_pending(true);
                return Action::Assassination(target);
            }
        }

        if context.coins >= 3
            && (context.cards.contains(&Card::Assassin)
                || self.bluff_ev_ok(context, Card::Assassin, 1.35, 1.8))
            && self.should_attempt_assassination(context)
        {
            self.set_assassination_pending(true);
            return Action::Assassination(target);
        }

        if context.cards.contains(&Card::Duke) || self.bluff_ev_ok(context, Card::Duke, 1.10, 2.0) {
            self.set_assassination_pending(false);
            return Action::Tax;
        }

        if opp.coins >= 2
            && (context.cards.contains(&Card::Captain)
                || self.bluff_ev_ok(context, Card::Captain, 1.05, 1.5)
                || (self.opponent_coup_threat_next_turn(context)
                    && self.bluff_ev_ok(context, Card::Captain, 1.25, 2.0)))
        {
            self.set_assassination_pending(false);
            return Action::Stealing(opp.name.clone());
        }

        if Self::remaining_copies(context, Card::Duke) >= 2 && context.history.len() % 3 == 0 {
            self.set_assassination_pending(false);
            return Action::ForeignAid;
        }

        self.set_assassination_pending(false);
        Action::Income
    }
}
