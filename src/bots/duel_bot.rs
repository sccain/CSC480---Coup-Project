use crate::{
    bot::{BotInterface, Context},
    Action, Card, History,
};

use std::cell::RefCell;

const N_CARDS: usize = 5;

#[derive(Default)]
struct Memory {
    seen_history_len: usize,
    opp_claims: [u32; N_CARDS],

    // For detecting "assassinate -> blocked" patterns
    my_assassination_pending: bool,
    assassination_blocked_streak: u32,
}

thread_local! {
    static MEMORY: RefCell<Memory> = RefCell::new(Memory::default());
}

pub struct DuelBot;

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

impl DuelBot {
    // Memory and history

    fn update_from_history(context: &Context) {
        // Reset memory at the start of a new game
        if context.history.is_empty() {
            MEMORY.with(|m| *m.borrow_mut() = Memory::default());
            return;
        }

        MEMORY.with(|m| {
            let mut mem = m.borrow_mut();

            if mem.seen_history_len >= context.history.len() {
                return;
            }

            // 1v1 opponent name
            let opp_name = context
                .playing_bots
                .iter()
                .find(|b| b.name != context.name)
                .unwrap()
                .name
                .clone();

            for h in &context.history[mem.seen_history_len..] {
                match h {
                    // Opponent claims
                    History::ActionTax { by } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Duke)] += 1;
                    }
                    History::ActionAssassination { by, .. } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Assassin)] += 1;
                    }
                    History::ActionStealing { by, .. } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Captain)] += 1;
                    }
                    History::ActionSwapping { by } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Ambassador)] += 1;
                    }

                    // Opponents claims for counters (blocks)
                    History::CounterForeignAid { by, .. } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Duke)] += 1;
                    }
                    History::CounterAssassination { by, .. } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Contessa)] += 1;

                        // If we had an assassination pending, it was blocked
                        if mem.my_assassination_pending {
                            mem.assassination_blocked_streak =
                                mem.assassination_blocked_streak.saturating_add(1);
                            mem.my_assassination_pending = false;
                        }
                    }
                    History::CounterStealing { by, .. } if *by == opp_name => {
                        // Steal block could be Captain OR Ambassador; count both as soft claims
                        mem.opp_claims[card_idx(Card::Captain)] += 1;
                        mem.opp_claims[card_idx(Card::Ambassador)] += 1;
                    }

                    // ----- Our actions: track assassination pending + reset streak on other actions -----
                    History::ActionAssassination { by, .. } if *by == context.name => {
                        mem.my_assassination_pending = true;
                    }
                    History::ActionTax { by } if *by == context.name => {
                        mem.my_assassination_pending = false;
                        mem.assassination_blocked_streak = 0;
                    }
                    History::ActionStealing { by, .. } if *by == context.name => {
                        mem.my_assassination_pending = false;
                        mem.assassination_blocked_streak = 0;
                    }
                    History::ActionSwapping { by } if *by == context.name => {
                        mem.my_assassination_pending = false;
                        mem.assassination_blocked_streak = 0;
                    }

                    _ => {}
                }
            }

            mem.seen_history_len = context.history.len();
        });
    }

    fn opp_claims(card: Card) -> u32 {
        MEMORY.with(|m| m.borrow().opp_claims[card_idx(card)])
    }

    fn assassination_blocked_streak() -> u32 {
        MEMORY.with(|m| m.borrow().assassination_blocked_streak)
    }

    fn set_assassination_pending(pending: bool) {
        MEMORY.with(|m| m.borrow_mut().my_assassination_pending = pending);
    }

    // -------- Card knowledge tables --------

    fn opponent<'a>(context: &'a Context) -> &'a crate::bot::OtherBot {
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

    // -------- Hypergeometric probability: P(opponent has >=1 of card) --------

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

    fn p_opponent_has(context: &Context, opp_cards: i32, card: Card) -> f64 {
        let n = Self::hidden_total(context);
        let k = Self::remaining_copies(context, card);

        if k == 0 || opp_cards <= 0 || n <= 0 {
            return 0.0;
        }

        let h = opp_cards.min(n);
        let base = 1.0 - (Self::n_choose_k(n - k, h) / Self::n_choose_k(n, h));

        // Credibility boost based on repeated claims
        let claims = Self::opp_claims(card) as f64;
        let mut odds = base / (1.0 - base + 1e-9);
        odds *= (0.35 * claims).exp();

        (odds / (1.0 + odds)).clamp(0.001, 0.999)
    }

    // -------- FULL probability distribution for bluffing --------
    //
    // Returns p_challenge. Then p_no_challenge = 1 - p_challenge.
    // This is an opponent model: "How likely are they to challenge this claim?"
    fn p_opponent_challenges_claim(context: &Context, claimed_role: Card, stake: f64) -> f64 {
        let opp = Self::opponent(context);

        let remaining = Self::remaining_copies(context, claimed_role).max(0) as f64; // 0..3
        if remaining <= 0.0 {
            return 0.999;
        }

        // Scarcity drives challenges
        let scarcity = 1.0 - (remaining / 3.0); // 0..1

        // If it's plausible THEY have the card, they may feel safer and challenge slightly less
        let p_opp_has_role = Self::p_opponent_has(context, opp.cards as i32, claimed_role);
        let cover_effect = (p_opp_has_role - 0.5) * -0.4;

        // If they are ahead, they can afford challenge risk more
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

    // EV-based bluff: compare "claim role action" vs safe fallback.
    // reward_delta: how much better than Income this is if not challenged.
    // stake: how important / conspicuous the claim is.
    fn bluff_ev_ok(context: &Context, role: Card, stake: f64, reward_delta: f64) -> bool {
        if Self::remaining_copies(context, role) == 0 {
            return false;
        }

        let p_chal = Self::p_opponent_challenges_claim(context, role, stake);
        let p_no = 1.0 - p_chal;

        let risk_loss = if context.cards.len() <= 1 { 2.0 } else { 1.0 }; // losing last influence is huge
        let ev = p_no * reward_delta - p_chal * risk_loss;

        ev > 0.10
    }

    // -------- Threat / desperation logic --------

    fn imminent_coup_loss(context: &Context) -> bool {
        // "lose next turn" in 1v1 typically means: we have 1 influence left,
        // opponent can coup on their next turn (coins >= 7).
        let opp = Self::opponent(context);
        context.cards.len() <= 1 && opp.coins >= 7
    }

    fn opponent_coup_threat_next_turn(context: &Context) -> bool {
        // Opp can coup next turn if they can reach 7 with a typical action.
        // Conservative check: if they already have 6 (Income -> 7) or 4 (Tax -> 7).
        let opp = Self::opponent(context);
        opp.coins >= 6 || opp.coins >= 4
    }

    // Should we attempt assassination this turn? (prevents "blocked forever")
    fn should_attempt_assassination(context: &Context) -> bool {
        if context.coins < 3 {
            return false;
        }

        let opp = Self::opponent(context);

        // Avoid donating 3 coins into a likely Contessa, especially after repeated blocks
        let streak = Self::assassination_blocked_streak();
        let p_contessa = Self::p_opponent_has(context, opp.cards as i32, Card::Contessa);

        if streak >= 1 && p_contessa > 0.55 {
            return false;
        }
        if streak >= 2 && p_contessa > 0.40 {
            return false;
        }

        true
    }

    fn should_challenge_contessa_block(context: &Context) -> bool {
        let opp = Self::opponent(context);

        // Don't spew challenges repeatedly after multiple blocks
        if Self::assassination_blocked_streak() >= 2 {
            return false;
        }

        if Self::remaining_copies(context, Card::Contessa) == 0 {
            return true;
        }

        let p_has = Self::p_opponent_has(context, opp.cards as i32, Card::Contessa);

        let v_win = 1.20;
        let mut v_lose = 1.00;
        if context.cards.len() <= 1 {
            v_lose *= 1.60;
        }

        (1.0 - p_has) * v_win > p_has * v_lose
    }

    // -------- Challenging action logic (with "must not lose next turn" boost) --------

    fn required_role_for_action(action: &Action) -> Option<Card> {
        match action {
            Action::Assassination(_) => Some(Card::Assassin),
            Action::Swapping => Some(Card::Ambassador),
            Action::Stealing(_) => Some(Card::Captain),
            Action::Tax => Some(Card::Duke),
            _ => None,
        }
    }

    fn should_challenge_action(context: &Context, _by: &str, action: &Action) -> bool {
        let Some(role) = Self::required_role_for_action(action) else { return false };

        if Self::remaining_copies(context, role) == 0 {
            return true;
        }

        let opp = Self::opponent(context);
        let p_has = Self::p_opponent_has(context, opp.cards as i32, role);

        // Base weights
        let (mut v_win, mut v_lose) = match action {
            Action::Assassination(_) => (1.25, 1.0),
            Action::Tax => (1.10, 1.0),
            Action::Swapping => (0.80, 1.0),
            _ => (1.0, 1.0),
        };

        // If we're on 1 influence, losing a challenge is worse.
        let life_factor = if context.cards.len() <= 1 { 1.6 } else { 1.0 };
        v_lose *= 1.10 * life_factor;

        // DESPERATION: if they are taxing and that likely means they'll coup next and we die,
        // be much more willing to challenge Duke.
        if matches!(action, Action::Tax) && context.cards.len() <= 1 && opp.coins >= 4 {
            v_win *= 2.2;
        }

        (1.0 - p_has) * v_win > p_has * v_lose
    }
}

impl BotInterface for DuelBot {
    fn get_name(&self) -> String {
        "DuelBot".to_string()
    }

    fn on_turn(&self, context: &Context) -> Action {
        Self::update_from_history(context);

        let opp = Self::opponent(context);
        let target = opp.name.clone();

        // If we can coup, do it.
        if context.coins >= 7 {
            Self::set_assassination_pending(false);
            return Action::Coup(target);
        }

        // If we will lose next turn to a coup, prioritize "stop coup" or "win now".
        if Self::imminent_coup_loss(context) {
            // Win now if possible: assassination can win immediately if opponent has 1 influence.
            if opp.cards <= 1 && context.coins >= 3
                && (context.cards.contains(&Card::Assassin) || Self::bluff_ev_ok(context, Card::Assassin, 1.45, 2.5))
                && Self::should_attempt_assassination(context)
            {
                Self::set_assassination_pending(true);
                return Action::Assassination(target);
            }

            // Otherwise prevent coup: steal (even as a bluff) if they have coins.
            if opp.coins >= 2 && (context.cards.contains(&Card::Captain) || Self::bluff_ev_ok(context, Card::Captain, 1.20, 2.0)) {
                Self::set_assassination_pending(false);
                return Action::Stealing(opp.name.clone());
            }

            // If we can't stop coup, take highest-variance "win chance": assassination bluff attempt if legal.
            if context.coins >= 3 && Self::bluff_ev_ok(context, Card::Assassin, 1.55, 2.2) && Self::should_attempt_assassination(context) {
                Self::set_assassination_pending(true);
                return Action::Assassination(target);
            }
        }

        // Assassination pressure (but avoid looping into blocks)
        if context.coins >= 3
            && (context.cards.contains(&Card::Assassin) || Self::bluff_ev_ok(context, Card::Assassin, 1.35, 1.8))
            && Self::should_attempt_assassination(context)
        {
            Self::set_assassination_pending(true);
            return Action::Assassination(target);
        }

        // Tax (real or EV-positive bluff)
        if context.cards.contains(&Card::Duke) || Self::bluff_ev_ok(context, Card::Duke, 1.10, 2.0) {
            Self::set_assassination_pending(false);
            return Action::Tax;
        }

        // Steal (real or EV-positive bluff), especially when opponent is close to coup range
        if opp.coins >= 2 && (context.cards.contains(&Card::Captain) || Self::bluff_ev_ok(context, Card::Captain, 1.05, 1.5)
            || (Self::opponent_coup_threat_next_turn(context) && Self::bluff_ev_ok(context, Card::Captain, 1.25, 2.0)))
        {
            Self::set_assassination_pending(false);
            return Action::Stealing(opp.name.clone());
        }

        // Opportunistic foreign aid
        if Self::remaining_copies(context, Card::Duke) >= 2 && context.history.len() % 3 == 0 {
            Self::set_assassination_pending(false);
            return Action::ForeignAid;
        }

        Self::set_assassination_pending(false);
        Action::Income
    }

    fn on_auto_coup(&self, context: &Context) -> String {
        Self::update_from_history(context);
        Self::opponent(context).name.clone()
    }

    fn on_challenge_action_round(&self, action: &Action, by: String, context: &Context) -> bool {
        Self::update_from_history(context);
        by != context.name && Self::should_challenge_action(context, &by, action)
    }

    fn on_counter(&self, action: &Action, _by: String, context: &Context) -> bool {
        Self::update_from_history(context);

        match action {
            Action::Assassination(_) => {
                if context.cards.contains(&Card::Contessa) {
                    true
                } else {
                    // Only bluff-block if the opponent isn't very likely to challenge it
                    let p_chal = Self::p_opponent_challenges_claim(context, Card::Contessa, 1.35);
                    Self::remaining_copies(context, Card::Contessa) > 0 && p_chal < 0.35
                }
            }

            Action::ForeignAid => {
                context.cards.contains(&Card::Duke)
                    || (Self::remaining_copies(context, Card::Duke) > 0
                        && Self::p_opponent_challenges_claim(context, Card::Duke, 1.05) < 0.40)
            }

            Action::Stealing(_) => {
                if context.cards.contains(&Card::Captain) || context.cards.contains(&Card::Ambassador) {
                    true
                } else {
                    let cap_ok = Self::remaining_copies(context, Card::Captain) > 0
                        && Self::p_opponent_challenges_claim(context, Card::Captain, 1.05) < 0.35;
                    let amb_ok = Self::remaining_copies(context, Card::Ambassador) > 0
                        && Self::p_opponent_challenges_claim(context, Card::Ambassador, 1.00) < 0.35;
                    cap_ok || amb_ok
                }
            }

            _ => false,
        }
    }

    fn on_challenge_counter_round(&self, action: &Action, _by: String, context: &Context) -> bool {
        Self::update_from_history(context);

        match action {
            // If they block our assassination, decide if we should challenge Contessa (EV-based, no repeat spew)
            Action::Assassination(_) => Self::should_challenge_contessa_block(context),

            // Hard-proof / safe checks
            Action::ForeignAid => Self::remaining_copies(context, Card::Duke) == 0,

            Action::Stealing(_) => {
                // They can block steal with Captain OR Ambassador.
                // In desperation (we die next turn), challenge more often if it might save us.
                if Self::imminent_coup_loss(context) {
                    // If either role is "rare", they're more likely bluffing.
                    let cap_rem = Self::remaining_copies(context, Card::Captain);
                    let amb_rem = Self::remaining_copies(context, Card::Ambassador);
                    (cap_rem == 0 && amb_rem == 0) || (cap_rem <= 1 && amb_rem <= 1)
                } else {
                    Self::remaining_copies(context, Card::Captain) == 0
                        && Self::remaining_copies(context, Card::Ambassador) == 0
                }
            }

            _ => false,
        }
    }

    fn on_swapping_cards(&self, new_cards: [Card; 2], context: &Context) -> [Card; 2] {
        Self::update_from_history(context);

        fn rank(c: Card) -> u8 {
            match c {
                Card::Duke => 5,
                Card::Assassin => 4,
                Card::Contessa => 3,
                Card::Captain => 2,
                Card::Ambassador => 1,
            }
        }

        let mut pool = vec![context.cards[0], context.cards[1], new_cards[0], new_cards[1]];
        pool.sort_by_key(|c| std::cmp::Reverse(rank(*c)));

        let keep1 = pool[0];
        let keep2 = pool[1];

        let mut discarded = Vec::new();
        let mut kept = 0;

        for c in [context.cards[0], context.cards[1], new_cards[0], new_cards[1]] {
            if kept < 2 && (c == keep1 || c == keep2) {
                kept += 1;
            } else {
                discarded.push(c);
            }
        }

        [discarded[0], discarded[1]]
    }

    fn on_card_loss(&self, context: &Context) -> Card {
        Self::update_from_history(context);

        fn rank(c: Card) -> u8 {
            match c {
                Card::Duke => 5,
                Card::Contessa => 4,
                Card::Assassin => 3,
                Card::Captain => 2,
                Card::Ambassador => 1,
            }
        }

        *context.cards.iter().min_by_key(|c| rank(**c)).unwrap()
    }
}

